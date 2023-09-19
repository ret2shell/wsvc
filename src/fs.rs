use std::path::{Path, PathBuf};

use blake3::{Hash, HexError};
use miniz_oxide::{deflate::compress_to_vec, inflate::decompress_to_vec};
use nanoid::nanoid;
use thiserror::Error;
use tokio::{
    fs::{create_dir_all, read_dir, remove_file, rename, write, File},
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::model::Record;

use super::model::{Blob, ObjectId, Repository, Tree};

#[derive(Error, Debug)]
pub enum WsvcFsError {
    #[error("os file system error")]
    Os(#[from] std::io::Error),
    #[error("invalid path")]
    InvalidPath,
    #[error("decompress error")]
    DecompressFailed(String),
    #[error("unknown path")]
    UnknownPath(String),
    #[error("invalid hex string")]
    InvalidHexString(#[from] HexError),
    #[error("unknown fs error")]
    Unknown,
    #[error("serialize failed")]
    SerializeFailed,
    #[error("deserialize failed")]
    DeserializeFailed,
    #[error("invalid filename")]
    InvalidFilename,
    #[error("invalid OsString")]
    InvalidOsString,
    #[error("dir already exists")]
    DirAlreadyExists,
}

#[derive(Clone, Debug)]
struct TreeImpl {
    name: String,
    trees: Vec<TreeImpl>,
    blobs: Vec<Blob>,
}

async fn store_blob_file_impl(
    path: impl AsRef<Path>,
    objects_dir: impl AsRef<Path>,
    temp: impl AsRef<Path>,
) -> Result<ObjectId, WsvcFsError> {
    if !temp.as_ref().exists() {
        create_dir_all(temp.as_ref()).await?;
    }
    let mut buffer: [u8; 1024] = [0; 1024];
    let mut file = File::open(&path).await?;
    let compressed_file_path = temp.as_ref().join(nanoid!());
    let mut compressed_file = File::create(&compressed_file_path).await?;
    let mut hasher = blake3::Hasher::new();
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        let compressed_data = compress_to_vec(&buffer[..n], 8);
        compressed_file
            .write_all(&[
                0x78,
                0xda,
                (&compressed_data.len() / 256).try_into().unwrap(),
                (&compressed_data.len() % 256).try_into().unwrap(),
            ])
            .await?;
        compressed_file.write_all(&compressed_data).await?;
    }
    let hash = hasher.finalize();
    let blob = objects_dir.as_ref().join(hash.to_hex().as_str());
    rename(&compressed_file_path, &blob).await?;
    Ok(ObjectId(hash))
}

async fn checkout_blob_file_impl(
    path: impl AsRef<Path>,
    objects_dir: impl AsRef<Path>,
    blob_hash: &ObjectId,
    temp: impl AsRef<Path>,
) -> Result<(), WsvcFsError> {
    let blob_path = objects_dir.as_ref().join(blob_hash.0.to_hex().as_str());
    let mut buffer: [u8; 2048] = [0; 2048];
    let mut header_buffer: [u8; 4] = [0; 4];
    let mut file = File::open(&blob_path).await?;
    let decompressed_file_path = temp.as_ref().join(nanoid!());
    let mut decompressed_file = File::create(&decompressed_file_path).await?;
    loop {
        let n = file.read(&mut header_buffer).await?;
        if n == 0 {
            break;
        }
        if header_buffer[0] != 0x78 || header_buffer[1] != 0xda {
            return Err(WsvcFsError::DecompressFailed(
                "magic header not match".to_owned(),
            ));
        }
        let size = (header_buffer[2] as usize) * 256 + (header_buffer[3] as usize);
        let n = file.read(&mut buffer[..size]).await?;
        if n != size {
            return Err(WsvcFsError::DecompressFailed("broken chunk".to_owned()));
        }
        decompressed_file
            .write_all(
                &decompress_to_vec(&buffer[..n])
                    .map_err(|_| WsvcFsError::DecompressFailed("decode chunk failed".to_owned()))?,
            )
            .await?;
    }
    rename(&decompressed_file_path, path).await?;
    Ok(())
}

#[async_recursion::async_recursion(?Send)]
async fn store_tree_file_impl(tree: TreeImpl, trees_dir: &Path) -> Result<Tree, WsvcFsError> {
    let mut result = Tree {
        name: tree.name,
        hash: ObjectId(Hash::from([0; 32])),
        trees: vec![],
        blobs: tree.blobs.clone(),
    };
    for tree in tree.trees {
        result
            .trees
            .push(store_tree_file_impl(tree, trees_dir.clone()).await?.hash);
    }
    let hash = blake3::hash(
        serde_json::to_vec(&result)
            .map_err(|_| WsvcFsError::SerializeFailed)?
            .as_slice(),
    );
    result.hash = ObjectId(hash);
    write(
        trees_dir.join(hash.to_string()),
        serde_json::to_vec(&result).map_err(|_| WsvcFsError::SerializeFailed)?,
    )
    .await?;

    Ok(result)
}

#[async_recursion::async_recursion(?Send)]
async fn build_tree(root: &Path, work_dir: &Path) -> Result<TreeImpl, WsvcFsError> {
    let mut result = TreeImpl {
        name: work_dir
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("."))
            .to_str()
            .ok_or(WsvcFsError::InvalidOsString)?
            .to_string(),
        trees: vec![],
        blobs: vec![],
    };
    let mut entries = read_dir(work_dir.clone()).await?;
    while let Some(entry) = entries.next_entry().await? {
        // println!("{:?}", entry);
        let entry_type = entry.file_type().await?;
        if entry_type.is_dir() {
            if entry.file_name() == ".wsvc" {
                continue;
            }
            result
                .trees
                .push(build_tree(root.clone(), &entry.path()).await?);
        } else if entry_type.is_file() {
            result.blobs.push(
                Blob {
                    name: entry
                        .file_name()
                        .to_str()
                        .ok_or(WsvcFsError::InvalidOsString)?
                        .to_string(),
                    hash: store_blob_file_impl(
                        &entry.path(),
                        &root.join("objects"),
                        &root.join("temp"),
                    )
                    .await?,
                }
                .clone(),
            );
        }
    }
    Ok(result)
}

impl Repository {
    pub async fn new(path: impl AsRef<Path>, is_bare: bool) -> Result<Self, WsvcFsError> {
        let mut path = path.as_ref().to_owned();
        if !is_bare {
            path = path.join(".wsvc");
        }
        if !path.exists() {
            create_dir_all(path.join("objects")).await?;
            create_dir_all(path.join("trees")).await?;
            create_dir_all(path.join("records")).await?;
            write(path.join("HEAD"), "").await?;
        } else {
            return Err(WsvcFsError::DirAlreadyExists);
        }
        Ok(Self { path })
    }

    pub async fn open(path: impl AsRef<Path>, is_bare: bool) -> Result<Self, WsvcFsError> {
        let mut path = path.as_ref().to_owned();
        if !is_bare {
            path = path.join(".wsvc");
        }
        if !path.exists() {
            return Err(WsvcFsError::UnknownPath(
                path.to_str()
                    .ok_or(WsvcFsError::InvalidOsString)?
                    .to_string(),
            ));
        }
        if path.join("objects").exists()
            && path.join("trees").exists()
            && path.join("records").exists()
            && path.join("HEAD").exists()
        {
            Ok(Self { path })
        } else {
            Err(WsvcFsError::UnknownPath(
                path.to_str()
                    .ok_or(WsvcFsError::InvalidOsString)?
                    .to_string(),
            ))
        }
    }

    pub async fn try_open(path: impl AsRef<Path>) -> Result<Self, WsvcFsError> {
        if let Ok(repo) = Repository::open(&path, false).await {
            Ok(repo)
        } else {
            Repository::open(&path, true).await
        }
    }

    pub async fn temp_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("temp");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    pub async fn objects_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("objects");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    pub async fn trees_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("trees");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    pub async fn records_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("records");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    pub async fn store_blob(
        &self,
        workspace: impl AsRef<Path>,
        rel_path: impl AsRef<Path>,
    ) -> Result<Blob, WsvcFsError> {
        Ok(Blob {
            name: rel_path
                .as_ref()
                .file_name()
                .ok_or(WsvcFsError::InvalidFilename)?
                .to_str()
                .ok_or(WsvcFsError::InvalidOsString)?
                .to_string(),
            hash: store_blob_file_impl(
                workspace.as_ref().join(rel_path),
                &self.objects_dir().await?,
                &self.temp_dir().await?,
            )
            .await?,
        })
    }

    pub async fn checkout_blob(
        &self,
        blob_hash: &ObjectId,
        workspace: impl AsRef<Path>,
        rel_path: impl AsRef<Path>,
    ) -> Result<(), WsvcFsError> {
        checkout_blob_file_impl(
            &workspace.as_ref().join(rel_path),
            &self.objects_dir().await?,
            &blob_hash,
            &self.temp_dir().await?,
        )
        .await
    }

    pub async fn read_blob(&self, blob_hash: &ObjectId) -> Result<Vec<u8>, WsvcFsError> {
        let blob_path = self
            .objects_dir()
            .await?
            .join(blob_hash.0.to_hex().as_str());
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut header_buffer: [u8; 4] = [0; 4];
        let mut file = File::open(&blob_path).await?;
        let mut result = Vec::new();
        loop {
            let n = file.read(&mut header_buffer).await?;
            if n == 0 {
                break;
            }
            if header_buffer[0] != 0x78 || header_buffer[1] != 0xda {
                return Err(WsvcFsError::DecompressFailed(
                    "magic header not match".to_owned(),
                ));
            }
            let size = (header_buffer[2] as usize) * 256 + (header_buffer[3] as usize);
            let n = file.read(&mut buffer[..size]).await?;
            if n != size {
                return Err(WsvcFsError::DecompressFailed("broken chunk".to_owned()));
            }
            result
                .extend_from_slice(&decompress_to_vec(&buffer[..n]).map_err(|_| {
                    WsvcFsError::DecompressFailed("decode chunk failed".to_owned())
                })?);
        }
        Ok(result)
    }

    pub async fn write_tree_recursively(
        &self,
        workspace: impl AsRef<Path> + Clone,
    ) -> Result<Tree, WsvcFsError> {
        let stored_tree = build_tree(&self.path, workspace.as_ref()).await?;
        let result = store_tree_file_impl(stored_tree, &self.trees_dir().await?).await?;
        Ok(result)
    }

    pub async fn read_tree(&self, tree_hash: &ObjectId) -> Result<Tree, WsvcFsError> {
        let tree_path = self.trees_dir().await?.join(tree_hash.0.to_hex().as_str());
        let result = serde_json::from_slice::<Tree>(&tokio::fs::read(tree_path).await?)
            .map_err(|_| WsvcFsError::DeserializeFailed)?;
        Ok(result)
    }

    #[async_recursion::async_recursion(?Send)]
    pub async fn checkout_tree(&self, tree: &Tree, workspace: &Path) -> Result<(), WsvcFsError> {
        // collect files to be deleted
        // delete files that not in the tree or hash not match
        let mut entries = read_dir(workspace).await?;
        while let Some(entry) = entries.next_entry().await? {
            let entry_type = entry.file_type().await?;
            if entry_type.is_file() {
                let mut found = false;
                for blob in &tree.blobs {
                    if blob.name
                        == entry
                            .file_name()
                            .to_str()
                            .ok_or(WsvcFsError::InvalidOsString)?
                    {
                        if !blob.checksum(workspace.join(&blob.name)).await? {
                            remove_file(workspace.join(&blob.name)).await?;
                        }
                        found = true;
                        break;
                    }
                }
                if !found {
                    remove_file(
                        workspace.join(
                            entry
                                .file_name()
                                .to_str()
                                .ok_or(WsvcFsError::InvalidOsString)?,
                        ),
                    )
                    .await?;
                }
            }
        }

        // checkout trees
        for tree in &tree.trees {
            let tree = self.read_tree(tree).await?;
            let tree_path = workspace.join(&tree.name);
            if !tree_path.exists() {
                create_dir_all(&tree_path).await?;
            }
            self.checkout_tree(&tree, &tree_path).await?;
        }
        for blob in &tree.blobs {
            self.checkout_blob(&blob.hash, &workspace, &blob.name)
                .await?;
        }
        Ok(())
    }

    pub async fn store_record(&self, record: &Record) -> Result<(), WsvcFsError> {
        let record_path = self
            .records_dir()
            .await?
            .join(record.hash.0.to_hex().as_str());
        write(
            record_path,
            serde_json::to_vec(record).map_err(|_| WsvcFsError::SerializeFailed)?,
        )
        .await?;
        Ok(())
    }

    pub async fn commit_record(
        &self,
        workspace: &Path,
        author: impl AsRef<str>,
        message: impl AsRef<str>,
    ) -> Result<(), WsvcFsError> {
        let tree = self.write_tree_recursively(workspace).await?;
        let record = Record {
            hash: ObjectId(Hash::from([0; 32])),
            message: String::from(message.as_ref()),
            author: String::from(author.as_ref()),
            date: chrono::Utc::now(),
            root: tree.hash,
        };
        let hash = blake3::hash(
            serde_json::to_vec(&record)
                .map_err(|_| WsvcFsError::SerializeFailed)?
                .as_slice(),
        );
        let record = Record {
            hash: ObjectId(hash),
            ..record
        };
        // write record to HEAD
        self.store_record(&record).await?;
        write(self.path.join("HEAD"), hash.to_hex().to_string()).await?;
        Ok(())
    }

    pub async fn read_record(&self, record_hash: &ObjectId) -> Result<Record, WsvcFsError> {
        let record_path = self
            .records_dir()
            .await?
            .join(record_hash.0.to_hex().as_str());
        let result = serde_json::from_slice::<Record>(&tokio::fs::read(record_path).await?)
            .map_err(|_| WsvcFsError::DeserializeFailed)?;
        Ok(result)
    }

    pub async fn checkout_record(
        &self,
        record_hash: &ObjectId,
        workspace: &Path,
    ) -> Result<(), WsvcFsError> {
        let record = self.read_record(record_hash).await?;
        self.checkout_tree(&self.read_tree(&record.root).await?, workspace)
            .await?;
        // write record to HEAD
        write(self.path.join("HEAD"), record_hash.0.to_hex().to_string()).await?;
        Ok(())
    }

    pub async fn get_records(&self) -> Result<Vec<Record>, WsvcFsError> {
        let mut result = Vec::new();
        let mut entries = read_dir(self.records_dir().await?).await?;
        while let Some(entry) = entries.next_entry().await? {
            let entry_type = entry.file_type().await?;
            if entry_type.is_file() {
                result.push(
                    self.read_record(&ObjectId(Hash::from_hex(
                        entry
                            .file_name()
                            .to_str()
                            .ok_or(WsvcFsError::InvalidOsString)?,
                    )?))
                    .await?,
                );
            }
        }
        Ok(result)
    }

    pub async fn get_latest_record(&self) -> Result<Option<Record>, WsvcFsError> {
        let mut result: Option<Record> = None;
        let mut entries = read_dir(self.records_dir().await?).await?;
        while let Some(entry) = entries.next_entry().await? {
            let entry_type = entry.file_type().await?;
            if entry_type.is_file() {
                let record = self
                    .read_record(&ObjectId(Hash::from_hex(
                        entry
                            .file_name()
                            .to_str()
                            .ok_or(WsvcFsError::InvalidOsString)?,
                    )?))
                    .await?;
                if result.is_none() || result.as_ref().unwrap().date < record.date {
                    result = Some(record);
                }
            }
        }
        Ok(result)
    }
}

impl Blob {
    pub async fn checksum(&self, rel_path: impl AsRef<Path>) -> Result<bool, WsvcFsError> {
        let mut file = File::open(rel_path).await?;
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut hasher = blake3::Hasher::new();
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hasher.finalize() == self.hash.0)
    }
}
