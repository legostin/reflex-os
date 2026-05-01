use crate::memory::agents::envelope::Envelope;
use crate::memory::schema::{MemoryError, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex};
use tokio::time::timeout;

#[derive(Clone)]
pub struct MessageBus {
    tx: broadcast::Sender<Envelope>,
    persist: Arc<Mutex<Option<File>>>,
}

impl MessageBus {
    /// Create a new message bus. `persist_path` is normally
    /// `<project_root>/.reflex/agents/bus.jsonl` (matches `BUS_LOG`).
    /// When `Some`, the parent directory is created and the file is opened
    /// in append mode for a JSONL audit trail of every published envelope.
    pub fn new(persist_path: Option<PathBuf>) -> Self {
        let (tx, _) = broadcast::channel(256);
        let persist: Option<File> = match persist_path {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    Ok(f) => Some(File::from_std(f)),
                    Err(e) => {
                        eprintln!(
                            "MessageBus: failed to open persist file {:?}: {}",
                            path, e
                        );
                        None
                    }
                }
            }
            None => None,
        };
        Self {
            tx,
            persist: Arc::new(Mutex::new(persist)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }

    pub async fn send(&self, env: Envelope) -> Result<()> {
        let mut guard = self.persist.lock().await;
        if let Some(file) = guard.as_mut() {
            let mut line = serde_json::to_vec(&env)?;
            line.push(b'\n');
            file.write_all(&line).await?;
            file.flush().await?;
        }
        drop(guard);
        let _ = self.tx.send(env);
        Ok(())
    }

    pub async fn request(&self, env: Envelope, timeout_ms: u64) -> Result<Envelope> {
        let mut rx = self.tx.subscribe();
        let corr = env.id.clone();
        let intent = env.intent.clone();
        self.send(env).await?;
        let deadline = Duration::from_millis(timeout_ms);
        loop {
            match timeout(deadline, rx.recv()).await {
                Err(_) => {
                    return Err(MemoryError::Bus(format!(
                        "request timeout after {}ms for intent {}",
                        timeout_ms, intent
                    )));
                }
                Ok(Ok(reply)) => {
                    if reply.correlation_id.as_deref() == Some(corr.as_str()) {
                        return Ok(reply);
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    eprintln!("MessageBus: receiver lagged by {} messages", n);
                    continue;
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    return Err(MemoryError::Bus("bus closed".into()));
                }
            }
        }
    }

    pub async fn send_to(
        &self,
        from: &str,
        to: &str,
        intent: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.send(Envelope::new(from, to, intent, payload)).await
    }

    pub async fn request_to(
        &self,
        from: &str,
        to: &str,
        intent: &str,
        payload: serde_json::Value,
        timeout_ms: u64,
    ) -> Result<Envelope> {
        self.request(Envelope::new(from, to, intent, payload), timeout_ms)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::agents::envelope::Envelope;
    use serde_json::json;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn subscribe_before_send_receives_late_subscriber_does_not() {
        let bus = MessageBus::new(None);
        let mut early = bus.subscribe();

        let bus2 = bus.clone();
        let send_task = tokio::spawn(async move {
            sleep(Duration::from_millis(20)).await;
            bus2.send(Envelope::new("a", "b", "topic.turn", json!({"x": 1})))
                .await
                .unwrap();
        });

        let got = timeout(Duration::from_millis(500), early.recv())
            .await
            .expect("early subscriber timed out")
            .expect("recv ok");
        assert_eq!(got.from, "a");
        assert_eq!(got.intent, "topic.turn");

        send_task.await.unwrap();

        let mut late = bus.subscribe();
        let r = timeout(Duration::from_millis(50), late.recv()).await;
        assert!(r.is_err(), "late subscriber should not see prior message");
    }

    #[tokio::test]
    async fn request_response_round_trip() {
        let bus = MessageBus::new(None);
        let bus_responder = bus.clone();
        let mut rx = bus_responder.subscribe();

        let responder = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(env) => {
                        if env.intent == "memory.recall.request" {
                            let reply = env.reply(
                                "indexer",
                                "memory.recall.response",
                                json!({"ok": true}),
                            );
                            bus_responder.send(reply).await.unwrap();
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });

        let req = Envelope::new("recall", "indexer", "memory.recall.request", json!({"q": "hi"}));
        let reply = bus.request(req, 1000).await.expect("reply");
        assert_eq!(reply.intent, "memory.recall.response");
        assert_eq!(reply.from, "indexer");
        assert_eq!(reply.payload, json!({"ok": true}));

        responder.await.unwrap();
    }

    #[tokio::test]
    async fn request_times_out_when_no_responder() {
        let bus = MessageBus::new(None);
        let req = Envelope::new("a", "ghost", "memory.recall.request", json!({}));
        let err = bus.request(req, 50).await.unwrap_err();
        match err {
            MemoryError::Bus(msg) => {
                assert!(msg.contains("timeout"), "unexpected msg: {msg}");
                assert!(msg.contains("memory.recall.request"));
            }
            other => panic!("expected Bus timeout, got {other:?}"),
        }
    }
}
