use crate::memory::agents::envelope::{intents, Envelope};
use crate::memory::agents::MessageBus;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Default)]
pub struct AppBusBridge {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    subs: Mutex<HashMap<String, HashSet<String>>>,
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
