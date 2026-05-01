use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod intents {
    pub const RECALL_REQUEST: &str = "memory.recall.request";
    pub const RECALL_RESPONSE: &str = "memory.recall.response";
    pub const INDEX_REQUEST: &str = "memory.index.request";
    pub const INDEX_DONE: &str = "memory.index.done";
    pub const FACT_PROPOSED: &str = "memory.fact.proposed";
    pub const FACT_APPROVED: &str = "memory.fact.approved";
    pub const FACT_REJECTED: &str = "memory.fact.rejected";
    pub const TOPIC_TURN: &str = "topic.turn";
    pub const TOPIC_IDLE: &str = "topic.idle";
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    RecallRequest,
    RecallResponse,
    IndexRequest,
    IndexDone,
    FactProposed,
    FactApproved,
    FactRejected,
    TopicTurn,
    TopicIdle,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: String,
    pub from: String,
    pub to: String,
    pub intent: String,
    pub payload: Value,
    #[serde(default)]
    pub correlation_id: Option<String>,
    pub created_at_ms: u128,
}

impl Envelope {
    pub fn new(from: &str, to: &str, intent: &str, payload: Value) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Self {
            id: format!("env_{now}_{}", rand_suffix()),
            from: from.into(),
            to: to.into(),
            intent: intent.into(),
            payload,
            correlation_id: None,
            created_at_ms: now,
        }
    }

    pub fn reply(&self, from: &str, intent: &str, payload: Value) -> Self {
        let mut env = Envelope::new(from, &self.from, intent, payload);
        env.correlation_id = Some(self.id.clone());
        env
    }
}

fn rand_suffix() -> String {
    use std::time::SystemTime;
    let n = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:08x}", n)
}
