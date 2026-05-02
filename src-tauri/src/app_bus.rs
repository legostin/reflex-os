use crate::memory::agents::envelope::{intents, Envelope};
use crate::memory::agents::MessageBus;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const APP_EVENT_RING_LIMIT: usize = 200;

#[derive(Clone, Default)]
pub struct AppBusBridge {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    subs: Mutex<HashMap<String, HashSet<String>>>,
    events: Mutex<EventLog>,
}

#[derive(Default)]
struct EventLog {
    next_seq: u64,
    records: VecDeque<AppEventRecord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppEventRecord {
    pub seq: u64,
    pub ts_ms: u64,
    pub topic: String,
    pub from_app: String,
    pub data: serde_json::Value,
}

impl AppBusBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, app_id: &str, topics: &[String]) {
        let mut g = self.inner.subs.lock().unwrap();
        let entry = g.entry(app_id.to_string()).or_default();
        for t in topics {
            entry.insert(t.clone());
        }
    }

    pub fn unsubscribe(&self, app_id: &str, topics: &[String]) {
        let mut g = self.inner.subs.lock().unwrap();
        if let Some(entry) = g.get_mut(app_id) {
            for t in topics {
                entry.remove(t);
            }
            if entry.is_empty() {
                g.remove(app_id);
            }
        }
    }

    pub fn clear(&self, app_id: &str) {
        let mut g = self.inner.subs.lock().unwrap();
        g.remove(app_id);
    }

    pub fn subscriptions(&self, app_id: &str) -> Vec<String> {
        let g = self.inner.subs.lock().unwrap();
        let mut topics: Vec<String> = g
            .get(app_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default();
        topics.sort();
        topics
    }

    pub fn matches(&self, app_id: &str, topic: &str) -> bool {
        let g = self.inner.subs.lock().unwrap();
        match g.get(app_id) {
            Some(set) => set.contains("*") || set.contains(topic),
            None => false,
        }
    }

    pub fn matching_apps(&self, topic: &str) -> Vec<String> {
        let g = self.inner.subs.lock().unwrap();
        g.iter()
            .filter_map(|(app, set)| {
                if set.contains("*") || set.contains(topic) {
                    Some(app.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn record_event(
        &self,
        from_app: &str,
        topic: &str,
        data: serde_json::Value,
    ) -> AppEventRecord {
        let mut g = self.inner.events.lock().unwrap();
        g.next_seq = g.next_seq.saturating_add(1);
        let record = AppEventRecord {
            seq: g.next_seq,
            ts_ms: now_ms(),
            topic: topic.to_string(),
            from_app: from_app.to_string(),
            data,
        };
        if g.records.len() >= APP_EVENT_RING_LIMIT {
            g.records.pop_front();
        }
        g.records.push_back(record.clone());
        record
    }

    pub fn recent_events(
        &self,
        app_id: &str,
        topic: Option<&str>,
        limit: usize,
    ) -> Vec<AppEventRecord> {
        if limit == 0 {
            return Vec::new();
        }
        let subscriptions = {
            let g = self.inner.subs.lock().unwrap();
            g.get(app_id).cloned().unwrap_or_default()
        };
        let g = self.inner.events.lock().unwrap();
        let mut out = Vec::with_capacity(limit.min(APP_EVENT_RING_LIMIT));
        for record in g.records.iter().rev() {
            if topic.is_some_and(|filter| filter != record.topic.as_str()) {
                continue;
            }
            let visible = record.from_app == app_id
                || subscriptions.contains("*")
                || subscriptions.contains(&record.topic);
            if visible {
                out.push(record.clone());
                if out.len() >= limit.min(APP_EVENT_RING_LIMIT) {
                    break;
                }
            }
        }
        out.reverse();
        out
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

pub fn start(bridge: AppBusBridge, bus: MessageBus, app: AppHandle) {
    let mut rx = bus.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    if env.intent != intents::APP_EVENT {
                        continue;
                    }
                    let topic = env
                        .payload
                        .get("topic")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if topic.is_empty() {
                        continue;
                    }
                    let from_app = env
                        .payload
                        .get("from_app")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let data = env
                        .payload
                        .get("data")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let recipients = bridge.matching_apps(topic);
                    for recipient in recipients {
                        if recipient == from_app {
                            continue;
                        }
                        let event_name = format!("reflex://app-event/{recipient}");
                        let payload = json!({
                            "topic": topic,
                            "from_app": from_app,
                            "data": data,
                        });
                        if let Err(e) = app.emit(&event_name, &payload) {
                            eprintln!("[app-bus] emit {event_name} failed: {e}");
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                Err(_) => continue,
            }
        }
    });
}

pub async fn emit_event(
    bus: &MessageBus,
    from_app: &str,
    topic: &str,
    data: serde_json::Value,
) -> Result<(), String> {
    let env = Envelope::new(
        &format!("app:{from_app}"),
        "*",
        intents::APP_EVENT,
        json!({
            "topic": topic,
            "from_app": from_app,
            "data": data,
        }),
    );
    bus.send(env).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriptions_are_sorted() {
        let bridge = AppBusBridge::default();
        bridge.subscribe("reader", &["z".into(), "a".into()]);

        assert_eq!(bridge.subscriptions("reader"), vec!["a", "z"]);
    }

    #[test]
    fn recent_events_include_own_and_subscribed_topics() {
        let bridge = AppBusBridge::default();
        bridge.subscribe("reader", &["alpha".into()]);
        bridge.record_event("reader", "own", json!({"n": 1}));
        bridge.record_event("writer", "alpha", json!({"n": 2}));
        bridge.record_event("writer", "beta", json!({"n": 3}));

        let recent = bridge.recent_events("reader", None, 10);
        let topics: Vec<&str> = recent.iter().map(|event| event.topic.as_str()).collect();
        assert_eq!(topics, vec!["own", "alpha"]);

        let filtered = bridge.recent_events("reader", Some("alpha"), 10);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].from_app, "writer");
    }
}
