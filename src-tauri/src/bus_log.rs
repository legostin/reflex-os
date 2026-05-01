use crate::logs::{self, LogLevel};
use crate::memory::agents::envelope::{intents, Envelope};
use crate::memory::agents::MessageBus;
use tauri::AppHandle;

const PAYLOAD_PREVIEW_CHARS: usize = 200;

pub fn start(bus: MessageBus, app: AppHandle) {
    let mut rx = bus.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    let level = level_for(&env.intent);
                    let msg = format_envelope(&env);
                    logs::log_with(&app, level, "bus", msg);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    logs::log_with(
                        &app,
                        LogLevel::Warn,
                        "bus",
                        format!("logger lagged, dropped {n} envelopes"),
                    );
                }
            }
        }
    });
}

fn level_for(intent: &str) -> LogLevel {
    if intent.contains("error") || intent.ends_with(".rejected") {
        return LogLevel::Warn;
    }
    match intent {
        intents::APP_EVENT
        | intents::APP_ACTION_REQUEST
        | intents::APP_ACTION_RESPONSE
        | intents::SCHEDULER_FIRE
        | intents::TOPIC_TURN
        | intents::TOPIC_IDLE
        | intents::FACT_PROPOSED
        | intents::FACT_APPROVED
        | intents::FACT_REJECTED => LogLevel::Debug,
        i if i.starts_with("memory.") => LogLevel::Trace,
        _ => LogLevel::Debug,
    }
}

fn format_envelope(env: &Envelope) -> String {
    let preview = payload_preview(&env.payload);
    let corr = env
        .correlation_id
        .as_deref()
        .map(|c| format!(" corr={c}"))
        .unwrap_or_default();
    format!(
        "{intent} {from}->{to}{corr} {preview}",
        intent = env.intent,
        from = env.from,
        to = env.to,
        corr = corr,
        preview = preview,
    )
}

fn payload_preview(payload: &serde_json::Value) -> String {
    if payload.is_null() {
        return String::new();
    }
    let s = payload.to_string();
    if s.chars().count() <= PAYLOAD_PREVIEW_CHARS {
        s
    } else {
        let mut out: String = s.chars().take(PAYLOAD_PREVIEW_CHARS).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn level_mapping() {
        assert!(matches!(level_for("memory.recall.request"), LogLevel::Trace));
        assert!(matches!(level_for(intents::APP_EVENT), LogLevel::Debug));
        assert!(matches!(level_for(intents::SCHEDULER_FIRE), LogLevel::Debug));
        assert!(matches!(level_for("memory.fact.rejected"), LogLevel::Warn));
        assert!(matches!(level_for("custom.error"), LogLevel::Warn));
        assert!(matches!(level_for("custom.intent"), LogLevel::Debug));
    }

    #[test]
    fn preview_truncates() {
        let big = json!({"data": "x".repeat(500)});
        let p = payload_preview(&big);
        assert!(p.chars().count() <= PAYLOAD_PREVIEW_CHARS + 1);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn preview_null_empty() {
        assert_eq!(payload_preview(&serde_json::Value::Null), "");
    }

    #[test]
    fn format_includes_correlation() {
        let mut env = Envelope::new("a", "b", "topic.turn", json!({"x": 1}));
        env.correlation_id = Some("env_123".into());
        let s = format_envelope(&env);
        assert!(s.contains("topic.turn"));
        assert!(s.contains("a->b"));
        assert!(s.contains("corr=env_123"));
    }
}
