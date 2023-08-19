use chrono::serde::ts_seconds::{deserialize as from_ts, serialize as to_ts};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type ObjectId = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Blob {
    pub name: String,
    pub hash: ObjectId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tree {
    pub name: String,
    pub hash: ObjectId,
    pub trees: Vec<ObjectId>,
    pub blobs: Vec<ObjectId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub hash: ObjectId,
    pub message: String,
    pub author: String,
    #[serde(deserialize_with = "from_ts", serialize_with = "to_ts")]
    pub date: DateTime<Utc>,
    pub root: ObjectId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repository {
    #[serde(skip_serializing, skip_deserializing)]
    pub path: std::path::PathBuf,
    pub head: Option<ObjectId>,
    pub records: Vec<ObjectId>,
}
