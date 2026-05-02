use crate::apps;
use crate::project;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use tauri::{AppHandle, Manager};
use tokio::process::Command as TokioCommand;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingSuggestion {
    pub app_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSuggestion {
    pub name: String,
    pub description: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionPlan {
    #[serde(default)]
    pub use_existing: Vec<ExistingSuggestion>,
    #[serde(default)]
    pub create_new: Vec<NewSuggestion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuggestionResult {
    pub plan: SuggestionPlan,
    pub project_description: Option<String>,
    pub raw: Option<String>,
}

const PROMPT_TEMPLATE: &str = r#"You are a Reflex assistant. You inspect a new project description and the catalog of installed utilities, then decide what should be attached or created.

GOAL:
1. Which EXISTING utilities fit and should be linked.
2. Which NEW utilities should be created.
3. If two new utility ideas overlap heavily, merge them into ONE broader utility.

RULES:
- Do not suggest creating a new utility when a suitable existing one exists; put it in use_existing.
- Do NOT duplicate existing utilities in create_new.
- reason must be one short phrase explaining why.
- If nothing new is needed, return create_new = [].
- If none of the existing utilities fit, return use_existing = [].
- Be pragmatic: recommend what is actually useful to the project owner on day one. Do not create utilities just in case.

RESPONSE: STRICT JSON ONLY, WITH NO EXPLANATION AND NO MARKDOWN FENCES:
{
  "use_existing": [ {"app_id": "...", "reason": "..."} ],
  "create_new": [ {"name": "...", "description": "...", "reason": "..."} ]
}

PROJECT:
Name: {PROJECT_NAME}
Description: {PROJECT_DESCRIPTION}

CATALOG ALREADY LINKED TO THIS PROJECT:
{LINKED}

CATALOG OF OTHER INSTALLED UTILITIES:
{INSTALLED}
"#;

pub async fn suggest(app: AppHandle, project_id: String) -> Result<SuggestionResult, String> {
    let proj = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let description = proj
        .description
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();
    if description.is_empty() {
        return Ok(SuggestionResult {
            plan: SuggestionPlan {
                use_existing: Vec::new(),
                create_new: Vec::new(),
            },
            project_description: None,
            raw: None,
        });
    }

    let apps_listing = apps::list_apps(&app).map_err(|e| e.to_string())?;
    let linked_ids: std::collections::HashSet<&str> =
        proj.apps.iter().map(|s| s.as_str()).collect();
    let mut linked: Vec<Value> = Vec::new();
    let mut installed: Vec<Value> = Vec::new();
    for l in &apps_listing {
        let m = &l.manifest;
        let entry = json!({
            "id": m.id,
            "name": m.name,
            "description": m.description,
            "widgets": m.widgets.iter().map(|w| json!({
                "id": w.id,
                "name": w.name,
                "description": w.description,
            })).collect::<Vec<_>>(),
            "actions": m.actions.iter().map(|a| json!({
                "id": a.id,
                "name": a.name,
                "description": a.description,
                "params_schema": a.params_schema,
                "public": a.public,
            })).collect::<Vec<_>>(),
        });
        if linked_ids.contains(m.id.as_str()) {
            linked.push(entry);
        } else {
            installed.push(entry);
        }
    }

    let prompt = PROMPT_TEMPLATE
        .replace("{PROJECT_NAME}", &proj.name)
        .replace("{PROJECT_DESCRIPTION}", &description)
        .replace(
            "{LINKED}",
            &serde_json::to_string_pretty(&linked).unwrap_or_else(|_| "[]".into()),
        )
        .replace(
            "{INSTALLED}",
            &serde_json::to_string_pretty(&installed).unwrap_or_else(|_| "[]".into()),
        );

    let raw = run_codex(&app, &prompt).await?;
    let plan = parse_plan(&raw);

    Ok(SuggestionResult {
        plan,
        project_description: Some(description),
        raw: Some(raw),
    })
}

async fn run_codex(app: &AppHandle, prompt: &str) -> Result<String, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    let scratch = base.join("scratch");
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let out_path: PathBuf = scratch.join(format!("suggester-{}.txt", uuid::Uuid::new_v4().simple()));
    let cwd_str = scratch.to_string_lossy().into_owned();
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
            prompt,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("codex spawn: {e}"))?;

    if !result.status.success() {
        let _ = std::fs::remove_file(&out_path);
        return Err(format!("codex exit non-zero: {}", result.status));
    }

    let raw = std::fs::read_to_string(&out_path)
        .map_err(|e| format!("read suggester out: {e}"))?;
    let _ = std::fs::remove_file(&out_path);
    Ok(raw.trim().to_string())
}

pub fn parse_plan(raw: &str) -> SuggestionPlan {
    let trimmed = raw.trim();
    if let Ok(p) = serde_json::from_str::<SuggestionPlan>(trimmed) {
        return p;
    }
    let stripped = strip_fences(trimmed);
    if let Ok(p) = serde_json::from_str::<SuggestionPlan>(stripped) {
        return p;
    }
    if let (Some(start), Some(end)) = (stripped.find('{'), stripped.rfind('}')) {
        if end > start {
            if let Ok(p) = serde_json::from_str::<SuggestionPlan>(&stripped[start..=end]) {
                return p;
            }
        }
    }
    SuggestionPlan {
        use_existing: Vec::new(),
        create_new: Vec::new(),
    }
}

fn strip_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json").or_else(|| s.strip_prefix("```")) {
        let rest = rest.trim();
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
        return rest;
    }
    s
}

#[tauri::command]
pub async fn suggest_apps_for_project(
    app: AppHandle,
    project_id: String,
) -> Result<SuggestionResult, String> {
    suggest(app, project_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_json() {
        let raw = r#"{"use_existing":[{"app_id":"a","reason":"fits"}],"create_new":[]}"#;
        let p = parse_plan(raw);
        assert_eq!(p.use_existing.len(), 1);
        assert_eq!(p.use_existing[0].app_id, "a");
        assert!(p.create_new.is_empty());
    }

    #[test]
    fn parses_fenced_json() {
        let raw = "```json\n{\"use_existing\":[],\"create_new\":[{\"name\":\"X\",\"description\":\"D\",\"reason\":\"R\"}]}\n```";
        let p = parse_plan(raw);
        assert_eq!(p.create_new.len(), 1);
        assert_eq!(p.create_new[0].name, "X");
    }

    #[test]
    fn falls_back_on_garbage() {
        let p = parse_plan("this is not json at all");
        assert!(p.use_existing.is_empty());
        assert!(p.create_new.is_empty());
    }

    #[test]
    fn extracts_json_from_prose() {
        let raw =
            "Here is my answer:\n{\"use_existing\":[{\"app_id\":\"x\",\"reason\":\"y\"}],\"create_new\":[]}\nthanks";
        let p = parse_plan(raw);
        assert_eq!(p.use_existing.len(), 1);
    }
}
