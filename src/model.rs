use std::path::PathBuf;

use chrono::serde::ts_seconds::{deserialize as from_ts, serialize as to_ts};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// `ObjectId` stand for a hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectId(pub blake3::Hash);

impl Default for ObjectId {
    fn default() -> Self {
        ObjectId(blake3::Hash::from_bytes([0; 32]))
    }
}

impl Serialize for ObjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'d> Deserialize<'d> for ObjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        let s = String::deserialize(deserializer)?;
        let hash = blake3::Hash::from_hex(s).map_err(serde::de::Error::custom)?;
        Ok(ObjectId(hash))
    }
}

impl From<blake3::Hash> for ObjectId {
    fn from(hash: blake3::Hash) -> Self {
        ObjectId(hash)
    }
}

impl TryFrom<String> for ObjectId {
    type Error = blake3::HexError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let hash = blake3::Hash::from_hex(s)?;
        Ok(ObjectId(hash))
    }
}

impl TryFrom<&str> for ObjectId {
    type Error = blake3::HexError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let hash = blake3::Hash::from_hex(s)?;
        Ok(ObjectId(hash))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// `Blob` stand for an object.
pub struct Blob {
    pub name: String,
    pub hash: ObjectId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// `Tree` stand for a dir.
pub struct Tree {
    pub name: String,
    pub hash: ObjectId,
    pub trees: Vec<ObjectId>,
    pub blobs: Vec<Blob>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// `Record` stand for a commit.
pub struct Record {
    pub hash: ObjectId,
    pub message: String,
    pub author: String,
    #[serde(deserialize_with = "from_ts", serialize_with = "to_ts")]
    pub date: DateTime<Utc>,
    pub root: ObjectId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// `Repository` stand for a repo.
pub struct Repository {
    pub path: PathBuf,
    pub lock: String,
}
