//! Persisted, provider-independent AI review data. Credentials never belong here.
use crate::StoryId;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDocument {
    pub version: u32,
    pub next_id: u64,
    pub messages: Vec<Message>,
    pub snapshots: BTreeMap<u64, Snapshot>,
    pub suggestions: Vec<Suggestion>,
    pub task_status: String,
}
impl Default for AiDocument {
    fn default() -> Self {
        Self { version: 1, next_id: 1, messages: vec![], snapshots: BTreeMap::new(), suggestions: vec![], task_status: String::new() }
    }
}
impl AiDocument {
    pub fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub story: StoryId,
    pub text: Arc<String>,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: u64,
    pub snapshot: u64,
    pub story: StoryId,
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub replacement: String,
    pub reason: String,
    pub status: Status,
    /// Immutable provenance for exported reports (live positions may be rebased).
    pub source_start: usize,
    pub source_end: usize,
    pub context: String,
    pub page: Option<usize>,
    pub page_label: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pending,
    Accepted,
    Rejected,
    Stale,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "待确认",
            Self::Accepted => "已接受",
            Self::Rejected => "已拒绝",
            Self::Stale => "已失效",
        }
    }
}
