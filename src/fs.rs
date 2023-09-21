use std::path::{Path, PathBuf};

use blake3::{Hash, HexError};
use miniz_oxide::{deflate::compress_to_vec, inflate::decompress_to_vec};
use nanoid::nanoid;
use thiserror::Error;
use tokio::{
    fs::{create_dir_all, read, read_dir, remove_dir_all, remove_file, rename, write, File},
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::model::Record;

use super::model::{Blob, ObjectId, Repository, Tree};

pub struct RepoGuard {
    pub repo: Repository,
}

impl RepoGuard {
    pub async fn new(repo: &Repository) -> Result<Self, WsvcFsError> {
        repo.check_lock().await?;
        repo.lock().await?;
        Ok(Self { repo: repo.clone() })
    }
}

impl Drop for RepoGuard {
    fn drop(&mut self) {
        let repo = self.repo.clone();
        repo.unlock().ok();
    }
}

/// Error type for wsvc fs.
#[derive(Error, Debug)]
pub enum WsvcFsError {
    #[error("os file system error: {0}")]
    Os(#[from] std::io::Error),
    #[error("decompress error: {0}")]
    DecompressFailed(String),
    #[error("unknown path: {0}")]
    UnknownPath(String),
    #[error("invalid hex string: {0}")]
    InvalidHexString(#[from] HexError),
    #[error("serialize failed: {0}")]
    SerializationFailed(#[from] serde_json::Error),
    #[error("invalid filename: {0}")]
    InvalidFilename(String),
    #[error("invalid OsString: {0}")]
    InvalidOsString(String),
    #[error("dir already exists: {0}")]
    DirAlreadyExists(String),
    #[error("no changes with record: {0}")]
    NoChanges(String),
    #[error("workspace is locked")]
    WorkspaceLocked,
}

#[derive(Clone, Debug)]
struct TreeImpl {
    name: String,
    trees: Vec<TreeImpl>,
    blobs: Vec<Blob>,
}

/// Store a blob file to objects dir.
async fn store_blob_file_impl(
    path: impl AsRef<Path>,
    objects_dir: impl AsRef<Path>,
    temp: impl AsRef<Path>,
) -> Result<ObjectId, WsvcFsError> {
    if !temp.as_ref().exists() {
        create_dir_all(temp.as_ref()).await?;
    }
    let mut buffer: [u8; 16384] = [0; 16384];
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

/// Checkout a blob file from objects dir to path
async fn checkout_blob_file_impl(
    path: impl AsRef<Path>,
    objects_dir: impl AsRef<Path>,
    blob_hash: &ObjectId,
    temp: impl AsRef<Path>,
) -> Result<(), WsvcFsError> {
    let blob_path = objects_dir.as_ref().join(blob_hash.0.to_hex().as_str());
    let mut buffer: [u8; 32768] = [0; 32768];
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

/// Store a tree file to trees dir.
/// Return a tuple of `(tree, is_new)`.
/// `is_new` means whether the tree is new created.
///
/// Noticed that this function is recursive.
#[async_recursion::async_recursion(?Send)]
async fn store_tree_file_impl(
    tree: TreeImpl,
    trees_dir: &Path,
) -> Result<(Tree, bool), WsvcFsError> {
    let mut result = Tree {
        name: tree.name,
        hash: ObjectId(Hash::from([0; 32])),
        trees: vec![],
        blobs: tree.blobs.clone(),
    };
    for tree in tree.trees {
        result
            .trees
            .push(store_tree_file_impl(tree, trees_dir.clone()).await?.0.hash);
    }
    let hash = blake3::hash(serde_json::to_vec(&result)?.as_slice());
    result.hash = ObjectId(hash);
    let tree_file_path = trees_dir.join(hash.to_hex().as_str());
    if !tree_file_path.exists() {
        write(
            trees_dir.join(hash.to_string()),
            serde_json::to_vec(&result)?,
        )
        .await?;
        return Ok((result, true));
    }

    Ok((result, false))
}

/// Build a tree from a work dir.
///
/// all blobs will be stored to objects dir when building.
#[async_recursion::async_recursion(?Send)]
async fn build_tree(root: &Path, work_dir: &Path) -> Result<TreeImpl, WsvcFsError> {
    let mut result = TreeImpl {
        name: work_dir
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("."))
            .to_str()
            .ok_or(WsvcFsError::InvalidOsString(format!("{:?}", work_dir)))?
            .to_string(),
        trees: vec![],
        blobs: vec![],
    };
    let mut entries = read_dir(work_dir.clone()).await?;
    while let Some(entry) = entries.next_entry().await? {
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
                        .ok_or(WsvcFsError::InvalidOsString(format!("{:?}", entry)))?
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
    /// init a new repository at path.
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
            return Err(WsvcFsError::DirAlreadyExists(format!("{:?}", path)));
        }
        Ok(Self {
            path,
            lock: nanoid!(),
        })
    }

    /// open a repository at path.
    pub async fn open(path: impl AsRef<Path>, is_bare: bool) -> Result<Self, WsvcFsError> {
        let mut path = path.as_ref().to_owned();
        if !is_bare {
            path = path.join(".wsvc");
        }
        if !path.exists() {
            return Err(WsvcFsError::UnknownPath(
                path.to_str()
                    .ok_or(WsvcFsError::InvalidOsString(format!("{:?}", path)))?
                    .to_string(),
            ));
        }
        if path.join("objects").exists()
            && path.join("trees").exists()
            && path.join("records").exists()
            && path.join("HEAD").exists()
        {
            Ok(Self {
                path,
                lock: nanoid!(),
            })
        } else {
            Err(WsvcFsError::UnknownPath(
                path.to_str()
                    .ok_or(WsvcFsError::InvalidOsString(format!("{:?}", path)))?
                    .to_string(),
            ))
        }
    }

    /// lock repository for write.
    ///
    /// notice that the Repository struct is `Clone`able, so there is still risky when you
    /// cloned multiple Repository struct in different threads. it is recommended to construct
    /// `Repository` everytime when you need it, the `lock` function is just for convenience.
    pub async fn lock(&self) -> Result<(), WsvcFsError> {
        write(self.path.join("LOCK"), self.lock.clone()).await?;
        Ok(())
    }

    /// check if the repository is locked.
    pub async fn check_lock(&self) -> Result<(), WsvcFsError> {
        let lock_path = self.path.join("LOCK");
        if !lock_path.exists() {
            return Ok(());
        }
        let lock = read(&lock_path).await?;
        if String::from_utf8(lock)
            .map_err(|err| WsvcFsError::InvalidOsString(format!("{:?}", err)))?
            == self.lock
        {
            Ok(())
        } else {
            Err(WsvcFsError::WorkspaceLocked)
        }
    }

    /// unlock repository.
    pub fn unlock(&self) -> Result<(), WsvcFsError> {
        std::fs::remove_file(self.path.join("LOCK"))?;
        Ok(())
    }

    /// try open a repository at path.
    ///
    /// the `bare` option will be guessed.
    pub async fn try_open(path: impl AsRef<Path>) -> Result<Self, WsvcFsError> {
        if let Ok(repo) = Repository::open(&path, false).await {
            Ok(repo)
        } else {
            Repository::open(&path, true).await
        }
    }

    /// get the temp folder of the repository.
    pub async fn temp_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("temp");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    /// get the objects folder of the repository.
    pub async fn objects_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("objects");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    /// get the trees folder of the repository.
    pub async fn trees_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("trees");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    /// get the records folder of the repository.
    pub async fn records_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.path.join("records");
        if !result.exists() {
            create_dir_all(&result).await?;
        }
        Ok(result)
    }

    /// store a blob file from workspace to objects dir.
    pub async fn store_blob(
        &self,
        workspace: impl AsRef<Path>,
        rel_path: impl AsRef<Path>,
    ) -> Result<Blob, WsvcFsError> {
        Ok(Blob {
            name: rel_path
                .as_ref()
                .file_name()
                .ok_or(WsvcFsError::InvalidFilename(format!(
                    "{:?}",
                    rel_path.as_ref()
                )))?
                .to_str()
                .ok_or(WsvcFsError::InvalidOsString(format!(
                    "{:?}",
                    rel_path.as_ref()
                )))?
                .to_string(),
            hash: store_blob_file_impl(
                workspace.as_ref().join(rel_path),
                &self.objects_dir().await?,
                &self.temp_dir().await?,
            )
            .await?,
        })
    }

    /// checkout a blob file from objects dir to workspace.
    pub async fn checkout_blob(
        &self,
        blob_hash: &ObjectId,
        workspace: impl AsRef<Path>,
        rel_path: impl AsRef<Path>,
    ) -> Result<(), WsvcFsError> {
        checkout_blob_file_impl(
            &workspace.as_ref().join(rel_path),
            &self.objects_dir().await?,
            blob_hash,
            &self.temp_dir().await?,
        )
        .await
    }

    /// read blob data from objects database.
    pub async fn read_blob(&self, blob_hash: &ObjectId) -> Result<Vec<u8>, WsvcFsError> {
        let blob_path = self
            .objects_dir()
            .await?
            .join(blob_hash.0.to_hex().as_str());
        let mut buffer: [u8; 32768] = [0; 32768];
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

    /// write all trees of current workspace to trees dir.
    pub async fn write_tree_recursively(
        &self,
        workspace: impl AsRef<Path> + Clone,
    ) -> Result<(Tree, bool), WsvcFsError> {
        let stored_tree = build_tree(&self.path, workspace.as_ref()).await?;
        let result = store_tree_file_impl(stored_tree, &self.trees_dir().await?).await?;
        Ok(result)
    }

    /// read a tree object from trees dir
    pub async fn read_tree(&self, tree_hash: &ObjectId) -> Result<Tree, WsvcFsError> {
        let tree_path = self.trees_dir().await?.join(tree_hash.0.to_hex().as_str());
        let result = serde_json::from_slice::<Tree>(&tokio::fs::read(tree_path).await?)?;
        Ok(result)
    }

    /// checkout a tree to workspace.
    #[async_recursion::async_recursion(?Send)]
    pub async fn checkout_tree(&self, tree: &Tree, workspace: &Path) -> Result<(), WsvcFsError> {
        // collect files to be deleted
        // delete files that not in the tree or hash not match
        let mut entries = read_dir(workspace).await?;
        let mut should_be_del = vec![];
        while let Some(entry) = entries.next_entry().await? {
            should_be_del.push(entry.file_name());
        }

        for tree in &tree.trees {
            let tree = self.read_tree(tree).await?;
            let tree_path = workspace.join(&tree.name);
            if !tree_path.exists() {
                create_dir_all(&tree_path).await?;
            } else {
                if !tree_path.is_dir() {
                    remove_file(&tree_path).await?;
                }
                if let Some(pos) = should_be_del
                    .iter()
                    .position(|x| x == tree_path.file_name().unwrap_or_default())
                {
                    should_be_del.remove(pos);
                }
            }
            self.checkout_tree(&tree, &tree_path).await?;
        }
        for blob in &tree.blobs {
            let blob_path = workspace.join(&blob.name);
            if !blob_path.exists() || !blob.checksum(&blob_path).await? {
                self.checkout_blob(&blob.hash, &workspace, &blob.name)
                    .await?;
            }
            if let Some(pos) = should_be_del
                .iter()
                .position(|x| x == blob_path.file_name().unwrap_or_default())
            {
                should_be_del.remove(pos);
            }
        }
        for entry in should_be_del {
            let entry_path = workspace.join(entry);
            if entry_path.is_dir() {
                if entry_path.file_name().unwrap().eq(".wsvc") {
                    continue;
                }
                remove_dir_all(entry_path).await?;
            } else {
                remove_file(entry_path).await?;
            }
        }
        Ok(())
    }

    /// store a record to records dir.
    pub async fn store_record(&self, record: &Record) -> Result<(), WsvcFsError> {
        let record_path = self
            .records_dir()
            .await?
            .join(record.hash.0.to_hex().as_str());
        write(record_path, serde_json::to_vec(record)?).await?;
        Ok(())
    }

    /// find a record for the specified tree.
    pub async fn find_record_for_tree(
        &self,
        tree_id: &Hash,
    ) -> Result<Option<Record>, WsvcFsError> {
        let records = self.get_records().await?;
        for record in records {
            let tree = self.read_tree(&record.root).await?;
            if tree.hash.0 == *tree_id {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    /// commit a record.
    pub async fn commit_record(
        &self,
        workspace: &Path,
        author: impl AsRef<str>,
        message: impl AsRef<str>,
    ) -> Result<Record, WsvcFsError> {
        let tree = self.write_tree_recursively(workspace).await?;
        if !tree.1 {
            if let Some(record) = self.find_record_for_tree(&tree.0.hash.0).await? {
                return Err(WsvcFsError::NoChanges(
                    record.hash.0.to_hex().to_owned().to_string(),
                ));
            }
        }
        let record = Record {
            hash: ObjectId(Hash::from([0; 32])),
            message: String::from(message.as_ref()),
            author: String::from(author.as_ref()),
            date: chrono::Utc::now(),
            root: tree.0.hash,
        };
        let hash = blake3::hash(serde_json::to_vec(&record)?.as_slice());
        let record = Record {
            hash: ObjectId(hash),
            ..record
        };
        // write record to HEAD
        self.store_record(&record).await?;
        write(self.path.join("HEAD"), hash.to_hex().to_string()).await?;
        Ok(record)
    }

    /// read a record from records dir.
    pub async fn read_record(&self, record_hash: &ObjectId) -> Result<Record, WsvcFsError> {
        let record_path = self
            .records_dir()
            .await?
            .join(record_hash.0.to_hex().as_str());
        let result = serde_json::from_slice::<Record>(&tokio::fs::read(record_path).await?)?;
        Ok(result)
    }

    /// checkout a record to workspace.
    pub async fn checkout_record(
        &self,
        record_hash: &ObjectId,
        workspace: &Path,
    ) -> Result<Record, WsvcFsError> {
        let record = self.read_record(record_hash).await?;
        self.checkout_tree(&self.read_tree(&record.root).await?, workspace)
            .await?;
        // write record to HEAD
        write(self.path.join("HEAD"), record_hash.0.to_hex().to_string()).await?;
        remove_dir_all(self.temp_dir().await?).await?;
        Ok(record)
    }

    /// get all records
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
                            .ok_or(WsvcFsError::InvalidOsString(format!("{:?}", entry)))?,
                    )?))
                    .await?,
                );
            }
        }
        Ok(result)
    }

    /// get all trees of a record
    pub async fn get_trees_of_record(
        &self,
        record_hash: &ObjectId,
    ) -> Result<Vec<Tree>, WsvcFsError> {
        let record = self.read_record(record_hash).await?;
        let mut result = Vec::new();
        let mut queue = vec![record.root];
        while let Some(tree_hash) = queue.pop() {
            let tree = self.read_tree(&tree_hash).await?;
            result.push(tree.clone());
            for tree_hash in tree.trees {
                queue.push(tree_hash);
            }
        }
        Ok(result)
    }

    /// get the latest record
    pub async fn get_latest_record(&self) -> Result<Option<Record>, WsvcFsError> {
        let mut records = self.get_records().await?;
        if records.is_empty() {
            return Ok(None);
        }
        records.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(Some(records[0].clone()))
    }

    /// get the head record
    pub async fn get_head_record(&self) -> Result<Option<Record>, WsvcFsError> {
        let head_hash = read(self.path.join("HEAD")).await?;
        if String::from_utf8(head_hash.clone())
            .map_err(|err| WsvcFsError::InvalidOsString(format!("{:?}", err)))?
            == *""
        {
            return Ok(None);
        }
        Ok(Some(
            self.read_record(
                &String::from_utf8(head_hash)
                    .map_err(|err| WsvcFsError::InvalidOsString(format!("{:?}", err)))?
                    .try_into()?,
            )
            .await?,
        ))
    }
}

impl Blob {
    /// get the checksum of a blob file.
    pub async fn checksum(&self, rel_path: impl AsRef<Path>) -> Result<bool, WsvcFsError> {
        let mut file = File::open(rel_path).await?;
        let mut buffer: [u8; 16384] = [0; 16384];
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
