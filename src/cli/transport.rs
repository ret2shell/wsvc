use std::path::Path;

use colored::Colorize;
use futures::{SinkExt, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::{create_dir_all, rename, write, File},
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{self, tungstenite, MaybeTlsStream, WebSocketStream};
use wsvc::{
    fs::{RepoGuard, WsvcFsError},
    model::{Blob, Record, Repository, Tree},
    WsvcError,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RecordWithState {
    pub record: Record,
    /// 0: same, 1: wanted, 2: will-give
    pub state: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TreeWithState {
    pub tree: Tree,
    /// 0: same, 1: wanted, 2: will-give
    pub state: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlobWithState {
    pub blob: Blob,
    /// 0: same, 1: wanted, 2: will-give
    pub state: i32,
}

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
        .map_err(WsvcError::NetworkError)?;
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
        .map_err(WsvcError::NetworkError)?;
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
        .map_err(WsvcError::NetworkError)?;
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
            .map_err(WsvcError::NetworkError)?;
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

async fn sync_records(
    repo: &Repository,
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<(Vec<Record>, Vec<Record>), WsvcError> {
    println!("{} {}", "[+]".bright_green(), "Sync records...".bold());
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.bold.green}    {wide_msg}")
            .unwrap()
            .tick_chars("* "),
    );
    pb.set_message("Receiving server records...");
    let server_records = recv_data(ws).await?;
    let server_records: Vec<Record> = serde_json::from_slice(&server_records)?;
    pb.set_message("Counting local records...");
    let local_records = repo.get_records().await?;
    pb.set_message("Differing records...");
    let wanted_records = server_records
        .iter()
        .filter(|r| !local_records.contains(r))
        .cloned()
        .collect::<Vec<_>>();
    let will_give_records = local_records
        .iter()
        .filter(|r| !server_records.contains(r))
        .cloned()
        .collect::<Vec<_>>();
    let mut response_records: Vec<RecordWithState> = wanted_records
        .iter()
        .map(|r| RecordWithState {
            record: r.clone(),
            state: 1,
        })
        .collect();
    response_records.extend_from_slice(
        &will_give_records
            .iter()
            .map(|r| RecordWithState {
                record: r.clone(),
                state: 2,
            })
            .collect::<Vec<RecordWithState>>(),
    );
    pb.set_message("Sending diff records...");
    let packet_body = serde_json::to_string(&response_records)?;
    send_data(ws, packet_body.into_bytes()).await?;
    pb.finish_and_clear();
    Ok((wanted_records, will_give_records))
}

async fn sync_trees(
    repo: &Repository,
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    given_records: &[Record],
) -> Result<(Vec<Tree>, Vec<Tree>), WsvcError> {
    println!("{} {}", "[+]".bright_green(), "Sync trees...".bold());
    let tick_style = ProgressStyle::with_template("{spinner:.bold.green}    {wide_msg}")
        .unwrap()
        .tick_chars("* ");
    let pb = ProgressBar::new_spinner();
    pb.set_style(tick_style.clone());
    pb.set_message("Receiving server trees...");
    let server_trees = recv_data(ws).await?;
    pb.set_message(format!(
        "Counting local trees for record... (0/{})",
        given_records.len()
    ));
    let server_trees: Vec<Tree> = serde_json::from_slice(&server_trees)?;
    let mut local_trees: Vec<Tree> = Vec::new();
    let mut i = 0;
    for record in given_records.iter() {
        i += 1;
        pb.set_message(format!(
            "Counting local trees for record... ({i}/{})",
            given_records.len()
        ));
        local_trees.extend_from_slice(
            &repo
                .get_trees_of_record(&record.hash)
                .await
                .map_err(WsvcError::FsError)?,
        );
    }
    pb.set_message("Differing trees...");
    let mut wanted_trees = Vec::new();
    for tree in &server_trees {
        if !repo.tree_exists(&tree.hash).await? {
            wanted_trees.push(tree.clone());
        }
    }
    let mut will_give_trees = Vec::new();
    for tree in local_trees {
        if !server_trees.contains(&tree) {
            will_give_trees.push(tree);
        }
    }
    let mut response_trees: Vec<TreeWithState> = wanted_trees
        .iter()
        .map(|t| TreeWithState {
            tree: t.clone(),
            state: 1,
        })
        .collect();
    response_trees.extend_from_slice(
        &will_give_trees
            .iter()
            .map(|t| TreeWithState {
                tree: t.clone(),
                state: 2,
            })
            .collect::<Vec<TreeWithState>>(),
    );
    pb.set_message("Sending diff trees...");
    let packet_body = serde_json::to_string(&response_trees)?;
    send_data(ws, packet_body.into_bytes()).await?;
    pb.finish_and_clear();
    Ok((wanted_trees, will_give_trees))
}

async fn sync_blobs_meta(
    repo: &Repository,
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    given_trees: &[Tree],
) -> Result<(Vec<Blob>, Vec<Blob>), WsvcError> {
    println!("{} {}", "[+]".bright_green(), "Sync blobs meta...".bold());
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.bold.green}    {wide_msg}")
            .unwrap()
            .tick_chars("* "),
    );
    pb.set_message("Receiving server blobs...");
    let server_blobs = recv_data(ws).await?;
    let server_blobs: Vec<Blob> = serde_json::from_slice(&server_blobs)?;
    pb.set_message(format!(
        "Counting local blobs for tree... (0/{})",
        given_trees.len()
    ));
    let mut i = 0;
    let mut local_blobs: Vec<Blob> = Vec::new();
    for tree in given_trees.iter() {
        i += 1;
        pb.set_message(format!(
            "Counting local blobs for tree... ({i}/{})",
            given_trees.len()
        ));
        local_blobs.extend_from_slice(
            &repo
                .get_blobs_of_tree(&tree.hash)
                .await
                .map_err(WsvcError::FsError)?,
        );
    }
    pb.set_message("Differing blobs...");
    let mut wanted_blobs = Vec::new();
    for blob in &server_blobs {
        if !repo.blob_exists(&blob.hash).await? {
            wanted_blobs.push(blob.clone());
        }
    }
    let mut will_give_blobs = Vec::new();
    for blob in local_blobs {
        if !server_blobs.contains(&blob) {
            will_give_blobs.push(blob);
        }
    }
    let mut response_blobs: Vec<BlobWithState> = wanted_blobs
        .iter()
        .map(|b| BlobWithState {
            blob: b.clone(),
            state: 1,
        })
        .collect();
    response_blobs.extend_from_slice(
        &will_give_blobs
            .iter()
            .map(|b| BlobWithState {
                blob: b.clone(),
                state: 2,
            })
            .collect::<Vec<BlobWithState>>(),
    );
    pb.set_message("Sending diff blobs...");
    let packet_body = serde_json::to_string(&response_blobs)?;
    send_data(ws, packet_body.into_bytes()).await?;
    pb.finish_and_clear();
    Ok((wanted_blobs, will_give_blobs))
}

async fn sync_blobs(
    repo: &Repository,
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    wanted_blobs: &[Blob],
    will_given_blobs: &[Blob],
) -> Result<(), WsvcError> {
    println!("{} {}", "[+]".bright_green(), "Sync blobs...".bold());
    let pb = ProgressBar::new(wanted_blobs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.yellow} {pos:>7}/{len:7} {msg}")
            .unwrap()
            .progress_chars("=>."),
    );
    let objects_dir = repo.objects_dir().await?;
    let temp_objects_dir = repo.temp_dir().await?.join("objects");
    if !temp_objects_dir.exists() {
        create_dir_all(&temp_objects_dir)
            .await
            .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }
    pb.set_message("Receiving...");
    pb.set_position(0);
    for _ in 0..wanted_blobs.len() {
        recv_file(ws, &temp_objects_dir).await?;
        pb.inc(1);
    }
    pb.set_message("Verifing...");
    pb.set_position(0);
    for i in wanted_blobs {
        let object_file = temp_objects_dir.join(i.hash.0.to_string());
        if !object_file.exists() {
            return Err(WsvcError::DataError(format!(
                "blob {} not synced from remote",
                i.hash.0.to_string()
            )));
        }
        pb.inc(1);
    }
    pb.finish_with_message("Done.");
    let pb = ProgressBar::new(will_given_blobs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.blue} {pos:>7}/{len:7} {msg}")
            .unwrap()
            .progress_chars("=>."),
    );
    pb.set_message("Sending...");
    pb.set_position(0);
    for blob in will_given_blobs {
        let object_file = objects_dir.join(blob.hash.0.to_string());
        let file = File::open(object_file)
            .await
            .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
        send_file(ws, &blob.hash.0.to_string(), file).await?;
        pb.inc(1);
    }
    pb.finish_with_message("Done.");
    let pb = ProgressBar::new(wanted_blobs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.green} {pos:>7}/{len:7} {msg}")
            .unwrap()
            .progress_chars("=>."),
    );
    pb.set_message("Moving...");
    for i in wanted_blobs {
        rename(
            temp_objects_dir.join(i.hash.0.to_string()),
            objects_dir.join(i.hash.0.to_string()),
        )
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
        pb.inc(1);
    }
    pb.finish_with_message("Done.");
    Ok(())
}

async fn sync_impl(repo: &Repository) -> Result<(), WsvcError> {
    let origin = repo.read_origin().await?;
    // the first round for client, receive server's all records
    println!(
        "{} {}",
        "[+]".bright_green(),
        "Connecting to remote server...".bold()
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(origin).await?;
    let (wanted_records, given_records) = sync_records(repo, &mut ws).await?;
    let (wanted_trees, given_trees) = sync_trees(repo, &mut ws, given_records.as_slice()).await?;
    let (wanted_blobs, given_blobs) =
        sync_blobs_meta(repo, &mut ws, given_trees.as_slice()).await?;
    sync_blobs(
        repo,
        &mut ws,
        wanted_blobs.as_slice(),
        given_blobs.as_slice(),
    )
    .await?;
    let trees_dir = repo.trees_dir().await.map_err(WsvcError::FsError)?;
    for tree in &wanted_trees {
        write(
            trees_dir.join(tree.hash.0.to_hex().as_str()),
            serde_json::to_string(tree)
                .map_err(|err| WsvcError::FsError(WsvcFsError::SerializationFailed(err)))?,
        )
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }
    let records_dir = repo.records_dir().await.map_err(WsvcError::FsError)?;
    println!("{} {}", "[*]".bright_blue(), "Summary:".bold());
    for record in &wanted_records {
        println!("  {} ({}) {}", "<<".bright_yellow(), record.hash.0.to_string()[0..6].dimmed().bold(), record.message);
        write(
            records_dir.join(record.hash.0.to_hex().as_str()),
            serde_json::to_string(record)
                .map_err(|err| WsvcError::FsError(WsvcFsError::SerializationFailed(err)))?,
        )
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }
    for record in &given_records {
        println!("  {} ({}) {}", ">>".bright_blue(), record.hash.0.to_string()[0..6].dimmed().bold(), record.message);
    }
    Ok(())
}

pub async fn clone(url: String, dir: Option<String>) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let repo_path = match dir {
        Some(p) => pwd.join(p),
        None => pwd.join(url.split('/').last().unwrap()),
    };
    let repo = Repository::new(&repo_path, false)
        .await
        .map_err(WsvcError::FsError)?;
    let guard = RepoGuard::new(&repo).await.map_err(WsvcError::FsError)?;
    repo.write_origin(url).await?;
    sync_impl(&repo).await?;
    let latest_record = repo
        .get_latest_record()
        .await
        .map_err(WsvcError::FsError)?
        .ok_or(WsvcError::EmptyRepoError)?;
    repo.checkout_record(&latest_record.hash, &repo_path)
        .await?;
    drop(guard);
    Ok(())
}

pub async fn sync() -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let repo = Repository::try_open(&pwd)
        .await
        .map_err(WsvcError::FsError)?;
    let guard = RepoGuard::new(&repo).await.map_err(WsvcError::FsError)?;
    sync_impl(&repo).await?;
    let latest_record = repo
        .get_latest_record()
        .await
        .map_err(WsvcError::FsError)?
        .ok_or(WsvcError::EmptyRepoError)?;
    repo.checkout_record(&latest_record.hash, pwd.as_path())
        .await?;
    drop(guard);
    Ok(())
}
