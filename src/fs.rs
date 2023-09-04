use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

use nanoid::nanoid;
use thiserror::Error;

use super::model::{Blob, ObjectId, Repository};

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
                .to_str()
                .ok_or(WsvcFsError::InvalidPath)?
                .to_string(),
            hash: ObjectId(hash),
        })
    }

    pub fn checkout_blob_file(
        &self,
        blob: &Blob,
        explicit_root: Option<impl AsRef<Path>>,
    ) -> Result<(), WsvcFsError> {
        if explicit_root.is_none() && self.bare {
            return Err(WsvcFsError::UnknownPath(
                "explicit root dir should be specified in bare repo".to_string(),
            ));
        }
        let explicit_root = explicit_root
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or_else(|| self.path.clone());
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
        let rel_path = PathBuf::from(blob.name.clone());
        let target_path = explicit_root.join(rel_path);
        if !target_path.exists() {
            std::fs::create_dir_all(target_path.parent().ok_or(WsvcFsError::InvalidPath)?)?;
        }
        std::fs::copy(&decompressed_file_path, &target_path)?;
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
}
