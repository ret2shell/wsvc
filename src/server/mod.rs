use std::path::Path;

use axum::extract::ws::{Message as AxumMessage, WebSocket};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    fs::{create_dir_all, read_dir, rename, write, File},
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::{
    fs::WsvcFsError,
    model::{Blob, Record, Repository, Tree},
    WsvcError,
};

/// `WsvcServerError` stand for server error.
#[derive(Error, Debug)]
pub enum WsvcServerError {
    #[error("fs error: {0}")]
    WsvcError(#[from] WsvcError),
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("network error: {0}")]
    NetworkError(#[from] axum::Error),
    #[error("data error: {0}")]
    DataError(String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct RecordWithState {
    pub record: Record,
    /// 0: same, 1: wanted, 2: will-give
    pub state: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BlobWithState {
    pub blob: Blob,
    /// 0: same, 1: wanted, 2: will-give
    pub state: i32,
}

async fn send_data(ws: &mut WebSocket, data: Vec<u8>) -> Result<(), WsvcServerError> {
    let mut header_buf = [0x33u8, 0x07u8, 0u8, 0u8, 0u8, 0u8];
    let size = data.len();
    header_buf[2] = (size >> 24) as u8;
    header_buf[3] = (size >> 16) as u8;
    header_buf[4] = (size >> 8) as u8;
    header_buf[5] = size as u8;
    ws.send(header_buf[..].into()).await?;
    // split data into 16384 bytes
    let mut offset = 0;
    while offset < data.len() {
        let end = offset + 16384;
        let end = if end > data.len() { data.len() } else { end };
        ws.send(data[offset..end].into()).await?;
        offset = end;
    }
    Ok(())
}

async fn recv_data(ws: &mut WebSocket) -> Result<Vec<u8>, WsvcServerError> {
    // match header and get size
    if let Some(Ok(AxumMessage::Binary(msg))) = ws.recv().await {
        let mut header_buf = [0u8; 6];
        header_buf.copy_from_slice(&msg[..6]);
        if header_buf[0] != 0x33 || header_buf[1] != 0x07 {
            return Err(WsvcServerError::DataError(
                "invalid packet header".to_owned(),
            ));
        }
        let size = ((header_buf[2] as usize) << 24)
            + ((header_buf[3] as usize) << 16)
            + ((header_buf[4] as usize) << 8)
            + (header_buf[5] as usize);
        let mut data = Vec::with_capacity(size);
        data.extend_from_slice(&msg[6..]);
        let mut offset = data.len();
        while offset < size {
            if let Some(Ok(AxumMessage::Binary(msg))) = ws.recv().await {
                data.extend_from_slice(&msg);
                offset = data.len();
            }
        }
        Ok(data)
    } else {
        Err(WsvcServerError::DataError(
            "invalid packet header".to_owned(),
        ))
    }
}

async fn send_file(
    ws: &mut WebSocket,
    file_name: &str,
    mut file: File,
) -> Result<(), WsvcServerError> {
    // file name packet header: 0x09 0x28 [size], 9.28 is Kamisato Ayaka's birthday
    let mut header_buf = [0x09u8, 0x28u8, 0u8, 0u8];
    let file_name_size = file_name.len();
    if file_name_size > 16384 {
        return Err(WsvcServerError::DataError("file name too long".to_owned()));
    }
    header_buf[2] = (file_name_size >> 8) as u8;
    header_buf[3] = file_name_size as u8;
    ws.send(header_buf[..].into()).await?;
    let mut file_header_buf = [0x07u8, 0x15u8, 0u8, 0u8, 0u8, 0u8];
    let mut buf = [0u8; 16384];
    let size = file
        .metadata()
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?
        .len() as usize;
    file_header_buf[2] = (size >> 24) as u8;
    file_header_buf[3] = (size >> 16) as u8;
    file_header_buf[4] = (size >> 8) as u8;
    file_header_buf[5] = size as u8;
    ws.send(file_header_buf[..].into()).await?;
    let mut offset = 0;
    while offset != size {
        let read_size = file
            .read(&mut buf)
            .await
            .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
        ws.send(buf[..read_size].into()).await?;
        offset += read_size;
    }
    Ok(())
}

async fn recv_file(
    ws: &mut WebSocket,
    storage_dir: impl AsRef<Path>,
) -> Result<(), WsvcServerError> {
    let file_name_header = ws
        .recv()
        .await
        .ok_or(WsvcServerError::DataError(format!(
            "invalid file name header: {}",
            "none"
        )))?
        .map_err(|err| WsvcServerError::NetworkError(err))?;
    let mut file_name_header_buf = [0u8; 4];
    if let AxumMessage::Binary(msg) = file_name_header {
        file_name_header_buf.copy_from_slice(&msg[..4]);
    } else {
        return Err(WsvcServerError::DataError(format!(
            "invalid file name header: {:?}",
            file_name_header
        )));
    }
    if file_name_header_buf[0] != 0x09 || file_name_header_buf[1] != 0x28 {
        return Err(WsvcServerError::DataError(format!(
            "invalid file name header: {:?}",
            file_name_header_buf
        )));
    }
    let file_name_size =
        ((file_name_header_buf[2] as usize) << 8) + (file_name_header_buf[3] as usize);
    let file_name = ws
        .recv()
        .await
        .ok_or(WsvcServerError::DataError(format!(
            "invalid file name: {}",
            "none"
        )))?
        .map_err(|err| WsvcServerError::NetworkError(err))?;
    let file_name = if let AxumMessage::Binary(msg) = file_name {
        String::from_utf8(msg[..file_name_size].to_vec())
            .map_err(|err| WsvcServerError::DataError(err.to_string()))?
    } else {
        return Err(WsvcServerError::DataError(format!(
            "invalid file name: {:?}",
            file_name
        )));
    };
    let file_path = storage_dir.as_ref().join(file_name);
    let file_header = ws
        .recv()
        .await
        .ok_or(WsvcServerError::DataError("invalid file header".to_owned()))?
        .map_err(|err| WsvcServerError::NetworkError(err))?;
    let mut file_header_buf = [0u8; 6];
    if let AxumMessage::Binary(msg) = file_header {
        file_header_buf.copy_from_slice(&msg[..6]);
    } else {
        return Err(WsvcServerError::DataError("invalid file header".to_owned()));
    }
    if file_header_buf[0] != 0x07 || file_header_buf[1] != 0x15 {
        return Err(WsvcServerError::DataError("invalid file header".to_owned()));
    }
    let size = ((file_header_buf[2] as usize) << 24)
        + ((file_header_buf[3] as usize) << 16)
        + ((file_header_buf[4] as usize) << 8)
        + (file_header_buf[5] as usize);
    let mut file = File::create(&file_path)
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let mut offset = 0;
    while offset != size {
        let data = ws
            .recv()
            .await
            .ok_or(WsvcServerError::DataError("invalid file data".to_owned()))?
            .map_err(|err| WsvcServerError::NetworkError(err))?;
        if let AxumMessage::Binary(data) = data {
            offset += data.len();
            file.write(&data)
                .await
                .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
        } else {
            return Err(WsvcServerError::DataError("invalid file data".to_owned()));
        }
    }

    Ok(())
}

/// `sync_with` syncs repository with client.
/// 
/// ## arguments
/// 
/// * `repo` - repository to sync with.
/// * `ws` - websocket connection from axum.
pub async fn sync_with(repo: Repository, mut ws: &mut WebSocket) -> Result<(), WsvcServerError> {
    // packet header: 0x33 0x07 [size]
    // the first round for server, pack all record and send it to client
    let records = repo.get_records().await.map_err(WsvcError::FsError)?;
    let packet_body = serde_json::to_string(&records)?;
    tracing::debug!("send records: {:?}", records);
    send_data(&mut ws, packet_body.into_bytes()).await?;
    let diff_records = recv_data(&mut ws).await?;
    tracing::debug!("recv diff records: {:?}", diff_records);
    let diff_records: Vec<RecordWithState> = serde_json::from_slice(&diff_records)?;
    let client_wanted_records = diff_records
        .iter()
        .filter(|r| r.state == 1)
        .map(|r| r.record.clone())
        .collect::<Vec<_>>();
    // do not store records until trees and blobs are synced.
    let client_will_given_records = diff_records
        .iter()
        .filter(|r| r.state == 2)
        .map(|r| r.record.clone())
        .collect::<Vec<_>>();

    // the second round for server, sync trees
    let mut trees = Vec::new();
    for record in &client_wanted_records {
        trees.extend_from_slice(
            &repo
                .get_trees_of_record(&record.hash)
                .await
                .map_err(WsvcError::FsError)?,
        );
    }
    let packet_body = serde_json::to_string(&trees)?;
    tracing::debug!("send trees: {:?}", trees);
    send_data(&mut ws, packet_body.into_bytes()).await?;

    // now client have the complete trees list.
    // in round three, server should send all blobs to client which are required.
    let new_trees = recv_data(&mut ws).await?;
    tracing::debug!("recv new trees: {:?}", new_trees);
    let new_trees: Vec<Tree> = serde_json::from_slice(&new_trees)?;
    let diff_blobs = recv_data(&mut ws).await?;
    tracing::debug!("recv diff blobs: {:?}", diff_blobs);
    let diff_blobs: Vec<BlobWithState> = serde_json::from_slice(&diff_blobs)?;
    let client_wanted_blobs = diff_blobs
        .iter()
        .filter(|b| b.state == 1)
        .map(|b| b.blob.clone())
        .collect::<Vec<_>>();
    let client_will_given_blobs = diff_blobs
        .iter()
        .filter(|b| b.state == 2)
        .map(|b| b.blob.clone())
        .collect::<Vec<_>>();

    // the third round for server, sync blobs
    for blob in client_wanted_blobs {
        let blob_path = repo
            .objects_dir()
            .await
            .map_err(WsvcError::FsError)?
            .join(blob.hash.0.to_hex().as_str());
        let file = File::open(blob_path)
            .await
            .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
        tracing::debug!("sending blob file: {:?}", blob.hash);
        send_file(&mut ws, &blob.hash.0.to_hex().as_str(), file).await?;
    }
    let temp_dir = repo.temp_dir().await.map_err(WsvcError::FsError)?;
    create_dir_all(temp_dir.join("objects"))
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let mut blob_count = client_will_given_blobs.len();
    let dir = temp_dir.join("objects");
    while blob_count > 0 {
        recv_file(&mut ws, &dir).await?;
        blob_count -= 1;
    }

    for blob in &client_will_given_blobs {
        tracing::debug!("checking blob file: {:?}", blob);
        if !blob
            .checksum(&dir.join(blob.hash.0.to_hex().as_str()))
            .await
            .map_err(WsvcError::FsError)?
        {
            return Err(WsvcServerError::DataError(format!(
                "blob {} checksum failed",
                blob.hash.0.to_hex().as_str()
            )));
        }
    }

    tracing::debug!("moving blob files to object database...");
    let objects_dir = repo.objects_dir().await.map_err(WsvcError::FsError)?;
    let mut entries = read_dir(&dir)
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?
    {
        rename(entry.path(), objects_dir.join(entry.file_name()))
            .await
            .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }

    // store trees
    tracing::debug!("write trees to tree database...");
    let trees_dir = repo.trees_dir().await.map_err(WsvcError::FsError)?;
    for tree in &new_trees {
        write(
            trees_dir.join(tree.hash.0.to_hex().as_str()),
            serde_json::to_string(tree)
                .map_err(|err| WsvcError::FsError(WsvcFsError::SerializationFailed(err)))?,
        )
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }

    // store records
    tracing::debug!("write records to record database...");
    let records_dir = repo.records_dir().await.map_err(WsvcError::FsError)?;
    for record in &client_will_given_records {
        write(
            records_dir.join(record.hash.0.to_hex().as_str()),
            serde_json::to_string(record)
                .map_err(|err| WsvcError::FsError(WsvcFsError::SerializationFailed(err)))?,
        )
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }

    Ok(())
}
