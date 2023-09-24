use std::path::{Path, PathBuf};

use futures::{SinkExt, StreamExt};
use tokio::{
    fs::{create_dir_all, read_dir, rename, write, File},
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{self, tungstenite, MaybeTlsStream, WebSocketStream};
use wsvc::{
    fs::{RepoGuard, WsvcFsError},
    model::{Record, Repository, Tree},
    server, WsvcError,
};

async fn send_data(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    data: Vec<u8>,
) -> Result<(), WsvcError> {
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

async fn recv_data(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<Vec<u8>, WsvcError> {
    if let Some(Ok(tungstenite::Message::Binary(msg))) = ws.next().await {
        let mut header_buf = [0u8; 6];
        header_buf.copy_from_slice(&msg[..6]);
        if header_buf[0] != 0x33 || header_buf[1] != 0x07 {
            return Err(WsvcError::DataError("invalid packet header".to_owned()));
        }
        let size = ((header_buf[2] as usize) << 24)
            + ((header_buf[3] as usize) << 16)
            + ((header_buf[4] as usize) << 8)
            + (header_buf[5] as usize);
        let mut data = Vec::with_capacity(size);
        data.extend_from_slice(&msg[6..]);
        let mut offset = data.len();
        while offset < size {
            if let Some(Ok(tungstenite::Message::Binary(msg))) = ws.next().await {
                data.extend_from_slice(&msg);
                offset = data.len();
            }
        }
        Ok(data)
    } else {
        Err(WsvcError::DataError("invalid packet header".to_owned()))
    }
}

async fn send_file(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    file_name: &str,
    mut file: File,
) -> Result<(), WsvcError> {
    // file name packet header: 0x09 0x28 [size], 9.28 is Kamisato Ayaka's birthday
    let mut header_buf = [0x09u8, 0x28u8, 0u8, 0u8];
    let file_name_size = file_name.len();
    if file_name_size > 16384 {
        return Err(WsvcError::DataError("file name too long".to_owned()));
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
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    storage_dir: impl AsRef<Path>,
) -> Result<(), WsvcError> {
    let file_name_header = ws
        .next()
        .await
        .ok_or(WsvcError::DataError(format!(
            "invalid file name header: {}",
            "none"
        )))?
        .map_err(|err| WsvcError::NetworkError(err))?;
    let mut file_name_header_buf = [0u8; 4];
    if let tungstenite::Message::Binary(msg) = file_name_header {
        file_name_header_buf.copy_from_slice(&msg[..4]);
    } else {
        return Err(WsvcError::DataError(format!(
            "invalid file name header: {:?}",
            file_name_header
        )));
    }
    if file_name_header_buf[0] != 0x09 || file_name_header_buf[1] != 0x28 {
        return Err(WsvcError::DataError(format!(
            "invalid file name header: {:?}",
            file_name_header_buf
        )));
    }
    let file_name_size =
        ((file_name_header_buf[2] as usize) << 8) + (file_name_header_buf[3] as usize);
    let file_name = ws
        .next()
        .await
        .ok_or(WsvcError::DataError(format!(
            "invalid file name: {}",
            "none"
        )))?
        .map_err(|err| WsvcError::NetworkError(err))?;
    let file_name = if let tungstenite::Message::Binary(msg) = file_name {
        String::from_utf8(msg[..file_name_size].to_vec())
            .map_err(|err| WsvcError::DataError(err.to_string()))?
    } else {
        return Err(WsvcError::DataError(format!(
            "invalid file name: {:?}",
            file_name
        )));
    };
    let file_path = storage_dir.as_ref().join(file_name);
    let file_header = ws
        .next()
        .await
        .ok_or(WsvcError::DataError("invalid file header".to_owned()))?
        .map_err(|err| WsvcError::NetworkError(err))?;
    let mut file_header_buf = [0u8; 6];
    if let tungstenite::Message::Binary(msg) = file_header {
        file_header_buf.copy_from_slice(&msg[..6]);
    } else {
        return Err(WsvcError::DataError("invalid file header".to_owned()));
    }
    if file_header_buf[0] != 0x07 || file_header_buf[1] != 0x15 {
        return Err(WsvcError::DataError("invalid file header".to_owned()));
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
            .next()
            .await
            .ok_or(WsvcError::DataError("invalid file data".to_owned()))?
            .map_err(|err| WsvcError::NetworkError(err))?;
        if let tungstenite::Message::Binary(data) = data {
            offset += data.len();
            file.write(&data)
                .await
                .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
        } else {
            return Err(WsvcError::DataError("invalid file data".to_owned()));
        }
    }

    Ok(())
}

async fn sync_impl(repo: &Repository) -> Result<(), WsvcError> {
    let origin = repo.read_origin().await?;
    // the first round for client, receive server's all records
    let (mut ws, _) = tokio_tungstenite::connect_async(origin).await?;
    let packet_body = recv_data(&mut ws).await?;
    let remote_records: Vec<Record> = serde_json::from_slice(&packet_body)?;
    tracing::debug!("recv records: {:?}", remote_records);
    let local_records = repo
        .get_records()
        .await
        .map_err(|err| WsvcError::FsError(err))?;
    let client_want_records_with_state = remote_records
        .iter()
        .filter(|r| !local_records.contains(r))
        .map(|r| server::RecordWithState {
            record: r.clone(),
            state: 1,
        })
        .collect::<Vec<_>>();
    let client_will_give_records_with_state = local_records
        .iter()
        .filter(|r| !remote_records.contains(r))
        .map(|r| server::RecordWithState {
            record: r.clone(),
            state: 2,
        })
        .collect::<Vec<_>>();
    let mut diff_records = Vec::with_capacity(
        client_want_records_with_state.len() + client_will_give_records_with_state.len(),
    );
    diff_records.extend_from_slice(&client_want_records_with_state);
    diff_records.extend_from_slice(&client_will_give_records_with_state);
    let packet_body = serde_json::to_string(&diff_records)?;
    send_data(&mut ws, packet_body.into_bytes()).await?;

    // the second round for client, sync trees
    let packet_body = recv_data(&mut ws).await?;
    let client_wanted_trees: Vec<Tree> = serde_json::from_slice(&packet_body)?;
    tracing::debug!("recv trees: {:?}", client_wanted_trees);
    let mut client_will_give_trees = Vec::new();
    for record_with_state in &client_want_records_with_state {
        client_will_give_trees.extend_from_slice(
            &repo
                .get_trees_of_record(&record_with_state.record.hash)
                .await
                .map_err(WsvcError::FsError)?,
        );
    }
    let packet_body = serde_json::to_string(&client_will_give_trees)?;
    send_data(&mut ws, packet_body.into_bytes()).await?;
    tracing::debug!("send trees: {:?}", client_will_give_trees);

    // the third round for client, sync blobs
    let mut client_wanted_blobs_with_states = Vec::new();
    for tree in &client_wanted_trees {
        client_wanted_blobs_with_states.extend_from_slice(
            &(tree
                .blobs
                .iter()
                .map(|b| server::BlobWithState {
                    blob: b.clone(),
                    state: 1,
                })
                .collect::<Vec<_>>()),
        )
    }
    let mut client_will_give_blobs_with_states = Vec::new();
    for tree in &client_will_give_trees {
        client_will_give_blobs_with_states.extend_from_slice(
            &(tree
                .blobs
                .iter()
                .map(|b| server::BlobWithState {
                    blob: b.clone(),
                    state: 2,
                })
                .collect::<Vec<_>>()),
        )
    }
    let mut diff_blobs = Vec::with_capacity(
        client_wanted_blobs_with_states.len() + client_will_give_blobs_with_states.len(),
    );
    diff_blobs.extend_from_slice(&client_wanted_blobs_with_states);
    diff_blobs.extend_from_slice(&client_will_give_blobs_with_states);
    tracing::debug!("send diff blobs: {:?}", diff_blobs);
    let packet_body = serde_json::to_vec(&diff_blobs)?;
    send_data(&mut ws, packet_body).await?;

    let temp_dir = repo.temp_dir().await.map_err(WsvcError::FsError)?;
    create_dir_all(temp_dir.join("objects"))
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let mut blob_count = client_wanted_blobs_with_states.len();
    let dir = temp_dir.join("objects");
    while blob_count > 0 {
        recv_file(&mut ws, &dir).await?;
        blob_count -= 1;
    }

    for blob_with_state in client_will_give_blobs_with_states {
        let blob_path = repo
            .objects_dir()
            .await
            .map_err(WsvcError::FsError)?
            .join(blob_with_state.blob.hash.0.to_hex().as_str());
        let file = File::open(blob_path)
            .await
            .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
        tracing::debug!("sending blob file: {:?}", blob_with_state.blob.hash);
        send_file(
            &mut ws,
            &blob_with_state.blob.hash.0.to_hex().as_str(),
            file,
        )
        .await?;
    }
    for blob_with_state in &client_wanted_blobs_with_states {
        tracing::debug!("checking blob file: {:?}", blob_with_state.blob);
        if !blob_with_state
            .blob
            .checksum(&dir.join(blob_with_state.blob.hash.0.to_hex().as_str()))
            .await
            .map_err(WsvcError::FsError)?
        {
            return Err(WsvcError::DataError(format!(
                "blob {} checksum failed",
                blob_with_state.blob.hash.0.to_hex().as_str()
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
    for tree in &client_wanted_trees {
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
    for record_with_state in &client_want_records_with_state {
        write(
            records_dir.join(record_with_state.record.hash.0.to_hex().as_str()),
            serde_json::to_string(&record_with_state.record)
                .map_err(|err| WsvcError::FsError(WsvcFsError::SerializationFailed(err)))?,
        )
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }
    Ok(())
}

pub async fn clone(_url: String, _dir: Option<String>) -> Result<(), WsvcError> {
    let repo_path = match _dir {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?,
    };
    let repo = Repository::new(repo_path, false)
        .await
        .map_err(|err| WsvcError::FsError(err))?;
    let guard = RepoGuard::new(&repo)
        .await
        .map_err(|err| WsvcError::FsError(err))?;
    repo.write_origin(_url).await?;
    sync_impl(&repo).await?;
    drop(guard);
    Ok(())
}

pub async fn sync() -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let repo = Repository::try_open(&pwd)
        .await
        .map_err(|err| WsvcError::FsError(err))?;
    let guard = RepoGuard::new(&repo)
        .await
        .map_err(|err| WsvcError::FsError(err))?;
    sync_impl(&repo).await?;
    let latest_record = repo
        .get_latest_record()
        .await
        .map_err(|err| WsvcError::FsError(err))?
        .ok_or(WsvcError::EmptyRepoError)?;
    repo.checkout_record(&latest_record.hash, &pwd.as_path())
        .await?;
    drop(guard);
    Ok(())
}
