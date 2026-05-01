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

const PROMPT_TEMPLATE: &str = r#"Ты — помощник Reflex, который смотрит на описание нового проекта и каталог уже установленных утилит, и решает что подойдёт.

ЦЕЛЬ:
1. Какие СУЩЕСТВУЮЩИЕ утилиты подходят и должны быть привязаны.
2. Какие НОВЫЕ утилиты стоит создать.
3. Если две идеи новой утилиты сильно перекрываются — объединяй их в ОДНУ более крупную.

ПРАВИЛА:
- Не предлагай создавать новую утилиту, если уже есть подходящая existing — выбирай её в use_existing.
- НЕ дублируй уже существующие в create_new.
- reason — одна короткая фраза почему.
- Если ничего нового не нужно — create_new = [].
- Если ничего из существующих не подходит — use_existing = [].
- Думай прагматично: что реально полезно владельцу проекта в первый день. Не плоди утилиты на всякий случай.

ОТВЕТ — СТРОГО JSON, БЕЗ ПОЯСНЕНИЙ И БЕЗ MARKDOWN-ФЕНСОВ:
{
  "use_existing": [ {"app_id": "...", "reason": "..."} ],
  "create_new": [ {"name": "...", "description": "...", "reason": "..."} ]
}

ПРОЕКТ:
Имя: {PROJECT_NAME}
Описание: {PROJECT_DESCRIPTION}

КАТАЛОГ УЖЕ ПРИВЯЗАННЫХ К ЭТОМУ ПРОЕКТУ:
{LINKED}

КАТАЛОГ ОСТАЛЬНЫХ УСТАНОВЛЕННЫХ УТИЛИТ:
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
        let raw = r#"{"use_existing":[{"app_id":"a","reason":"подходит"}],"create_new":[]}"#;
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
        let p = parse_plan("это не json совсем");
        assert!(p.use_existing.is_empty());
        assert!(p.create_new.is_empty());
    }

    #[test]
    fn extracts_json_from_prose() {
        let raw =
            "Вот мой ответ:\n{\"use_existing\":[{\"app_id\":\"x\",\"reason\":\"y\"}],\"create_new\":[]}\nспасибо";
        let p = parse_plan(raw);
        assert_eq!(p.use_existing.len(), 1);
    }
}
