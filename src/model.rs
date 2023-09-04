use chrono::serde::ts_seconds::{deserialize as from_ts, serialize as to_ts};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer, Deserializer};

#[derive(Clone, Debug)]
pub struct ObjectId(blake3::Hash);

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
        let hash = blake3::Hash::from_hex(&s).map_err(serde::de::Error::custom)?;
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
    pub blobs: Vec<ObjectId>,
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
    #[serde(skip_serializing, skip_deserializing)]
    pub path: std::path::PathBuf,
    pub head: Option<ObjectId>,
    pub records: Vec<ObjectId>,
}
