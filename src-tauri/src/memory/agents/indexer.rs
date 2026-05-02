use crate::memory::agents::envelope::{intents, Envelope};
use crate::memory::agents::MessageBus;
use crate::memory::rag;
use crate::memory::schema::{MemoryError, MemoryKind, MemoryScope, Result, ScopeRoots};
use crate::memory::store::{self, SaveRequest};
use crate::storage;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command as TokioCommand;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::sleep;

const CHECK_INTERVAL_MS: u64 = 5_000;
const MAX_TRANSCRIPT_CHARS: usize = 12_000;
const MAX_RAW_PER_EVENT_CHARS: usize = 500;

const EXTRACT_PROMPT_TEMPLATE: &str = r#"You are the agent memory indexer. Read the dialogue transcript below and return a STRICTLY valid JSON array of facts worth remembering for future sessions in this project. Do not run commands. Do not include explanations.

Each fact:
{
  "kind": "user|project|feedback|reference|tool|fact",
  "name": "<3-7 words>",
  "description": "<one line, 5-15 words>",
  "body": "<1-3 short paragraphs in the source dialogue language>",
  "tags": ["..."]
}

Remember only information that:
- cannot be trivially inferred from git or code (do not describe the file structure),
- may be useful in future conversations,
- is stable, not a momentary plan step.

If nothing is found, return [].

Transcript:
---
{TRANSCRIPT}
---"#;

#[derive(Debug, Clone)]
pub struct IndexerConfig {
    pub idle_debounce_ms: u64,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            idle_debounce_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParsedFact {
    #[serde(default)]
    pub kind: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn run(
    bus: MessageBus,
    project_root: PathBuf,
    thread_id: String,
    cfg: IndexerConfig,
) -> Result<()> {
    let mut receiver = bus.subscribe();
    let mut last_turn_at: Option<Instant> = None;
    let mut pending = false;
    let debounce = Duration::from_millis(cfg.idle_debounce_ms);
    let check = Duration::from_millis(CHECK_INTERVAL_MS);

    loop {
        tokio::select! {
            recv = receiver.recv() => {
                match recv {
                    Ok(env) => {
                        if env.intent == intents::TOPIC_TURN && envelope_thread_matches(&env, &thread_id) {
                            last_turn_at = Some(Instant::now());
                            pending = true;
                        }
                    }
                    Err(RecvError::Closed) => return Ok(()),
                    Err(RecvError::Lagged(_)) => continue,
                }
            }
            _ = sleep(check) => {
                if pending {
                    if let Some(last) = last_turn_at {
                        if last.elapsed() >= debounce {
                            match index_thread_once(&project_root, &thread_id).await {
                                Ok(count) => {
                                    let env = Envelope::new(
                                        &format!("indexer:{thread_id}"),
                                        "*",
                                        intents::INDEX_DONE,
                                        serde_json::json!({
                                            "thread_id": thread_id,
                                            "facts": count,
                                        }),
                                    );
                                    if let Err(e) = bus.send(env).await {
                                        if !matches!(e, MemoryError::Unimplemented(_)) {
                                            eprintln!("[reflex] indexer bus send err: {e}");
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[reflex] indexer run failed for {thread_id}: {e}");
                                }
                            }
                            pending = false;
                            last_turn_at = None;
                        }
                    }
                }
            }
        }
    }
}

fn envelope_thread_matches(env: &Envelope, thread_id: &str) -> bool {
    env.payload
        .get("thread_id")
        .and_then(|v| v.as_str())
        .map(|s| s == thread_id)
        .unwrap_or(false)
}

pub async fn index_thread_once(project_root: &Path, thread_id: &str) -> Result<usize> {
    let events = match storage::read_stored_events(project_root, thread_id) {
        Ok(e) => e,
        Err(e) => return Err(MemoryError::Other(format!("read_stored_events: {e}"))),
    };
    if events.is_empty() {
        return Ok(0);
    }

    let transcript = build_transcript(&events);
    let prompt = EXTRACT_PROMPT_TEMPLATE.replace("{TRANSCRIPT}", &transcript);

    let thread_dir = match storage::thread_dir(project_root, thread_id) {
        Ok(d) => d,
        Err(e) => return Err(MemoryError::Other(format!("thread_dir: {e}"))),
    };
    let out_path = thread_dir.join("indexer-out.json");
    let cwd_str = project_root.to_string_lossy().into_owned();
    let out_str = out_path.to_string_lossy().into_owned();

    let result = TokioCommand::new("codex")
        .args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-s",
            "read-only",
            "--output-last-message",
            &out_str,
            "-C",
            &cwd_str,
            "--",
            &prompt,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| MemoryError::Other(format!("codex spawn: {e}")))?;

    if !result.status.success() {
        let _ = std::fs::remove_file(&out_path);
        return Err(MemoryError::Other(format!(
            "codex exit non-zero: {}",
            result.status
        )));
    }

    let last_msg = std::fs::read_to_string(&out_path)
        .map_err(|e| MemoryError::Other(format!("read indexer-out: {e}")))?;

    let facts = parse_facts_response(&last_msg);
    let _ = std::fs::remove_file(&out_path);

    if facts.is_empty() {
        return Ok(0);
    }

    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let roots = ScopeRoots::resolve(&home, Some(project_root), Some(thread_id));

    let mut saved_count = 0usize;
    for fact in facts {
        let kind = parse_kind(fact.kind.as_deref()).unwrap_or(MemoryKind::Fact);
        let req = SaveRequest {
            scope: MemoryScope::Topic,
            kind,
            name: fact.name.clone(),
            description: fact.description.clone(),
            body: fact.body.clone(),
            rel_path: None,
            tags: fact.tags.clone(),
            source: Some(thread_id.to_string()),
        };
        match store::save(&roots, req) {
            Ok(note) => {
                let doc_id = format!("memory:{}", note.rel_path.display());
                if let Err(e) =
                    rag::index_text(project_root, &doc_id, "memory", &fact.body).await
                {
                    if !matches!(e, MemoryError::Unimplemented(_)) {
                        eprintln!("[reflex] indexer rag index err: {e}");
                    }
                }
                saved_count += 1;
            }
            Err(MemoryError::Unimplemented(_)) => continue,
            Err(e) => {
                eprintln!("[reflex] indexer save fact failed: {e}");
            }
        }
    }

    Ok(saved_count)
}

fn build_transcript(events: &[storage::StoredEvent]) -> String {
    let mut pieces: Vec<String> = Vec::new();
    let mut total = 0usize;
    for ev in events.iter().rev() {
        let piece = extract_event_text(&ev.raw);
        if piece.trim().is_empty() {
            continue;
        }
        let trimmed = if piece.chars().count() > MAX_RAW_PER_EVENT_CHARS {
            piece.chars().take(MAX_RAW_PER_EVENT_CHARS).collect::<String>()
        } else {
            piece
        };
        let line = format!("[{}] {}", ev.stream, trimmed);
        let len = line.chars().count();
        if total + len > MAX_TRANSCRIPT_CHARS {
            break;
        }
        total += len;
        pieces.push(line);
    }
    pieces.reverse();
    pieces.join("\n")
}

fn extract_event_text(raw: &str) -> String {
    let v: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => {
            return raw
                .chars()
                .take(MAX_RAW_PER_EVENT_CHARS)
                .collect::<String>();
        }
    };

    if let Some(t) = pluck_text(&v) {
        return t;
    }

    let s = v.to_string();
    s.chars().take(MAX_RAW_PER_EVENT_CHARS).collect()
}

fn pluck_text(v: &Value) -> Option<String> {
    let typ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let interesting = matches!(
        typ,
        "agent_message"
            | "user_message"
            | "agent_message_delta"
            | "assistant_message"
            | "thread.user_message"
            | "thread.agent_message"
    );
    if interesting {
        if let Some(s) = v.get("message").and_then(|x| x.as_str()) {
            return Some(format!("{typ}: {s}"));
        }
        if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
            return Some(format!("{typ}: {s}"));
        }
        if let Some(s) = v.get("content").and_then(|x| x.as_str()) {
            return Some(format!("{typ}: {s}"));
        }
        if let Some(arr) = v.get("content").and_then(|x| x.as_array()) {
            let joined: Vec<String> = arr
                .iter()
                .filter_map(|p| {
                    p.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            if !joined.is_empty() {
                return Some(format!("{typ}: {}", joined.join(" ")));
            }
        }
    }
    None
}

fn parse_kind(s: Option<&str>) -> Option<MemoryKind> {
    let s = s?.trim().to_ascii_lowercase();
    match s.as_str() {
        "user" => Some(MemoryKind::User),
        "project" => Some(MemoryKind::Project),
        "feedback" => Some(MemoryKind::Feedback),
        "reference" => Some(MemoryKind::Reference),
        "tool" => Some(MemoryKind::Tool),
        "system" => Some(MemoryKind::System),
        "fact" => Some(MemoryKind::Fact),
        _ => None,
    }
}

pub fn parse_facts_response(raw: &str) -> Vec<ParsedFact> {
    let candidate = strip_fences(raw.trim());
    if let Ok(v) = serde_json::from_str::<Vec<ParsedFact>>(&candidate) {
        return v;
    }
    if let (Some(start), Some(end)) = (candidate.find('['), candidate.rfind(']')) {
        if end > start {
            if let Ok(v) = serde_json::from_str::<Vec<ParsedFact>>(&candidate[start..=end]) {
                return v;
            }
        }
    }
    Vec::new()
}

fn strip_fences(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        let rest = rest.trim_start_matches('\n');
        if let Some(idx) = rest.rfind("```") {
            return rest[..idx].trim().to_string();
        }
        return rest.trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.trim_start_matches('\n');
        if let Some(idx) = rest.rfind("```") {
            return rest[..idx].trim().to_string();
        }
        return rest.trim().to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_facts_response_plain_array() {
        let raw = r#"[{"kind":"user","name":"Likes Russian","description":"d","body":"b","tags":["lang"]}]"#;
        let facts = parse_facts_response(raw);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].name, "Likes Russian");
        assert_eq!(facts[0].kind.as_deref(), Some("user"));
    }

    #[test]
    fn parse_facts_response_fenced_with_prose() {
        let raw = r#"Here is the JSON:
```json
[
  {"kind":"project","name":"Reflex MVP","description":"macOS agent","body":"Tauri 2 + React","tags":["tauri"]},
  {"kind":"fact","name":"Codex CLI usage","description":"how to call","body":"codex exec ...","tags":[]}
]
```
That's all."#;
        let facts = parse_facts_response(raw);
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[1].name, "Codex CLI usage");
    }

    #[test]
    fn parse_facts_response_empty_on_garbage() {
        let raw = "no json here";
        let facts = parse_facts_response(raw);
        assert!(facts.is_empty());
    }
}
