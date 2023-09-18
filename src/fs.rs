use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use nanoid::nanoid;
use thiserror::Error;

use super::model::{Blob, ObjectId, Record, Repository, Tree};

#[derive(Error, Debug)]
pub enum WsvcFsError {
    #[error("os file system error")]
    Os(#[from] std::io::Error),
    #[error("invalid path")]
    InvalidPath,
    #[error("decompress error")]
    DecompressFailed,
    #[error("unknown path")]
    UnknownPath(String),
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
}

impl Repository {
    pub fn root_dir(&self) -> PathBuf {
        if self.bare {
            self.path.clone()
        } else {
            self.path.join(".wsvc")
        }
    }

    pub fn tmp_dir(&self) -> Result<PathBuf, WsvcFsError> {
        let result = self.root_dir().join("tmp");
        if !result.exists() {
            std::fs::create_dir_all(&result)?;
        }
        Ok(result)
    }

    pub fn write_blob_file(&self, rel_path: impl AsRef<Path>) -> Result<Blob, WsvcFsError> {
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut file = std::fs::File::open(&rel_path)?;
        let compressed_file_path = self.tmp_dir()?.join(nanoid!());
        let mut compressed_file = std::fs::File::create(&compressed_file_path)?;
        let mut hasher = blake3::Hasher::new();
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
            compressed_file.write_all(&miniz_oxide::deflate::compress_to_vec(&buffer[..n], 8))?;
        }
        let hash = hasher.finalize();
        let blob = self.root_dir().join("objects").join(hash.to_hex().as_str());
        std::fs::copy(&compressed_file_path, &blob)?;
        Ok(Blob {
            name: rel_path
                .as_ref()
                .file_name()
                .ok_or(WsvcFsError::InvalidFilename)?
                .to_str()
                .ok_or(WsvcFsError::InvalidOsString)?
                .to_string(),
            hash: ObjectId(hash),
        })
    }

    pub fn checkout_blob_file(
        &self,
        blob: &Blob,
        rel_path: impl AsRef<Path>,
    ) -> Result<(), WsvcFsError> {
        let rel_path = rel_path.as_ref();
        let blob_path = self
            .root_dir()
            .join("objects")
            .join(blob.hash.0.to_hex().as_str());
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut file = std::fs::File::open(&blob_path)?;
        let decompressed_file_path = self.tmp_dir()?.join(nanoid!());
        let mut decompressed_file = std::fs::File::create(&decompressed_file_path)?;
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            decompressed_file.write_all(
                &miniz_oxide::inflate::decompress_to_vec(&buffer[..n])
                    .map_err(|_| WsvcFsError::DecompressFailed)?,
            )?;
        }
        std::fs::copy(&decompressed_file_path, rel_path)?;
        Ok(())
    }

    pub fn checkout_blob_data(&self, blob: &Blob) -> Result<Vec<u8>, WsvcFsError> {
        let blob_path = self
            .root_dir()
            .join("objects")
            .join(blob.hash.0.to_hex().as_str());
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut file = std::fs::File::open(&blob_path)?;
        let mut result = Vec::new();
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            result.extend_from_slice(
                &miniz_oxide::inflate::decompress_to_vec(&buffer[..n])
                    .map_err(|_| WsvcFsError::DecompressFailed)?,
            );
        }
        Ok(result)
    }

    pub fn write_tree_file(&self, rel_path: impl AsRef<Path>) -> Result<Tree, WsvcFsError> {
        let path = rel_path.as_ref();
        let entries = std::fs::read_dir(path)?;
        let mut trees = vec![];
        let mut blobs = vec![];

        for entry in entries {
            let entry = entry?;

            let entry_type = entry.file_type()?;
            if entry_type.is_dir() {
                if entry.file_name() == ".wsvc" {
                    continue;
                }
                trees.push(self.write_tree_file(entry.path())?.hash);
            } else if entry_type.is_file() {
                blobs.push(self.write_blob_file(entry.path())?);
            }
        }

        let name = path
            .file_name()
            .unwrap_or(std::ffi::OsStr::new(""))
            .to_str()
            .ok_or(WsvcFsError::InvalidOsString)?;

        let hash = blake3::hash(format!("{}:{:?}:{:?}", name, trees, blobs).as_bytes());

        let tree = Tree {
            name: name.to_string(),
            hash: ObjectId(hash),
            trees: trees,
            blobs: blobs,
        };

        std::fs::write(
            self.root_dir().join("trees").join(hash.to_string()),
            serde_json::to_vec(&tree).map_err(|_| WsvcFsError::SerializeFailed)?,
        )?;

        Ok(tree)
    }

    pub fn checkout_root_tree(
        &self,
        explicit_root: Option<impl AsRef<Path>>,
        tree_id: ObjectId,
    ) -> Result<(), WsvcFsError> {
        let explicit_root = self.get_explicit_root(explicit_root)?;

        let root_tree_file =
            std::fs::File::open(self.root_dir().join("trees").join(tree_id.0.to_string()))?;

        let root_tree: Tree =
            serde_json::from_reader(root_tree_file).map_err(|_| WsvcFsError::DeserializeFailed)?;

        self.checkout_tree(explicit_root, root_tree)?;

        Ok(())
    }

    fn checkout_subtree(
        &self,
        parent_path: impl AsRef<Path>,
        tree_id: ObjectId,
    ) -> Result<String, WsvcFsError> {
        let subtree_file =
            std::fs::File::open(self.root_dir().join("trees").join(tree_id.0.to_string()))?;

        let subtree: Tree =
            serde_json::from_reader(subtree_file).map_err(|_| WsvcFsError::DeserializeFailed)?;

        let subtree_path = parent_path.as_ref().join(&subtree.name);

        if !subtree_path.exists() {
            std::fs::create_dir(&subtree_path)?;
        }

        self.checkout_tree(subtree_path, subtree)
    }

    fn checkout_tree(
        &self,
        tree_path: impl AsRef<Path>,
        tree: Tree,
    ) -> Result<String, WsvcFsError> {
        let mut subtree_name_set = HashSet::with_capacity(tree.trees.len());
        for sub_tree_id in tree.trees {
            subtree_name_set.insert(self.checkout_subtree(&tree_path, sub_tree_id)?);
        }

        let mut blob_name_set: HashMap<_, _> = tree
            .blobs
            .into_iter()
            .map(|x| (x.name.clone(), x))
            .collect();

        let entries = std::fs::read_dir(&tree_path)?;

        for entry in entries {
            let entry = entry?;

            let entry_name = entry
                .file_name()
                .to_str()
                .ok_or(WsvcFsError::InvalidOsString)?
                .to_string();

            let entry_type = entry.file_type()?;

            if entry_type.is_dir() {
                if entry_name != ".wsvc" && !subtree_name_set.contains(&entry_name) {
                    std::fs::remove_dir_all(entry.path())?;
                }
            }

            if entry_type.is_file() {
                match blob_name_set.remove(&entry_name) {
                    Some(x) => {
                        if !x.verify(entry.path())? {
                            self.checkout_blob_file(&x, entry.path())?;
                        }
                    }
                    None => {
                        std::fs::remove_file(entry.path())?;
                    }
                }
            }
        }

        for b in blob_name_set {
            self.checkout_blob_file(&b.1, tree_path.as_ref().join(b.0))?;
        }

        Ok(tree.name)
    }

    fn get_explicit_root(
        &self,
        explicit_root: Option<impl AsRef<Path>>,
    ) -> Result<PathBuf, WsvcFsError> {
        if explicit_root.is_none() && self.bare {
            return Err(WsvcFsError::UnknownPath(
                "explicit root dir should be specified in bare repo".to_string(),
            ));
        }
        Ok(explicit_root
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or_else(|| self.path.clone()))
    }

    pub fn create_record(
        &self,
        explicit_root: Option<impl AsRef<Path>>,
        message: String,
        author: String,
    ) -> Result<Record, WsvcFsError> {
        let explicit_root = self.get_explicit_root(explicit_root)?;

        let root = self.write_tree_file(explicit_root)?.hash;

        let date = chrono::Utc::now();

        let hash = blake3::hash(format!("{}:{}:{:?}:{:?}", message, author, date, root).as_bytes());

        let record = Record {
            hash: ObjectId(hash),
            author,
            message,
            date,
            root,
        };

        std::fs::write(
            self.root_dir().join("records").join(hash.to_string()),
            serde_json::to_vec(&record).map_err(|_| WsvcFsError::SerializeFailed)?,
        )?;
        Ok(record)
    }
}

impl Blob {
    fn verify(&self, rel_path: impl AsRef<Path>) -> Result<bool, WsvcFsError> {
        let mut file = std::fs::File::open(rel_path)?;
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut hasher = blake3::Hasher::new();
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hasher.finalize() == self.hash.0)
    }
}
