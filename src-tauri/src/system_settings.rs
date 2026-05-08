use crate::app_server;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "system-settings.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemSettings {
    #[serde(default = "settings_version")]
    pub version: u32,
    #[serde(default)]
    pub request_profiles: RequestProfiles,
    #[serde(default)]
    pub disabled_skills: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestProfiles {
    #[serde(default)]
    pub complex: RequestProfile,
    #[serde(default)]
    pub fast: RequestProfile,
    #[serde(default)]
    pub instant: RequestProfile,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RequestProfile {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SystemSettingsPayload {
    pub settings: SystemSettings,
    pub models: Vec<CodexModel>,
    pub skills: Vec<CodexSkill>,
    pub codex_home: String,
    pub settings_path: String,
    pub codex_model: Option<String>,
    pub codex_reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CodexModel {
    pub slug: String,
    pub display_name: String,
    pub default_reasoning_level: Option<String>,
    pub supported_reasoning_levels: Vec<CodexReasoningLevel>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CodexReasoningLevel {
    pub effort: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CodexSkill {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub source: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug)]
pub enum RequestKind {
    Complex,
    Fast,
    Instant,
}

#[derive(Default)]
struct CodexDefaults {
    model: Option<String>,
    reasoning_effort: Option<String>,
    enabled_plugins: HashSet<String>,
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            version: settings_version(),
            request_profiles: RequestProfiles::default(),
            disabled_skills: Vec::new(),
        }
    }
}

impl Default for RequestProfiles {
    fn default() -> Self {
        Self {
            complex: RequestProfile {
                model: None,
                reasoning_effort: Some("high".into()),
            },
            fast: RequestProfile {
                model: None,
                reasoning_effort: Some("medium".into()),
            },
            instant: RequestProfile {
                model: None,
                reasoning_effort: Some("low".into()),
            },
        }
    }
}

fn settings_version() -> u32 {
    1
}

#[tauri::command]
pub fn system_settings_get(app: AppHandle) -> Result<SystemSettingsPayload, String> {
    payload(&app)
}

#[tauri::command]
pub fn system_settings_save(
    app: AppHandle,
    settings: SystemSettings,
) -> Result<SystemSettingsPayload, String> {
    let catalog = codex_catalog();
    let normalized = normalize_settings(settings, &catalog.models, &catalog.defaults);
    write_settings(&app, &normalized)?;
    payload(&app)
}

pub(crate) fn thread_overrides(app: &AppHandle, kind: RequestKind) -> app_server::ThreadOverrides {
    let catalog = codex_catalog();
    let settings = read_settings(app)
        .map(|value| normalize_settings(value, &catalog.models, &catalog.defaults))
        .unwrap_or_else(|_| default_settings(&catalog.models, &catalog.defaults));
    let profile = match kind {
        RequestKind::Complex => settings.request_profiles.complex,
        RequestKind::Fast => settings.request_profiles.fast,
        RequestKind::Instant => settings.request_profiles.instant,
    };
    app_server::ThreadOverrides {
        model: profile.model.and_then(non_empty_string),
        reasoning_effort: profile.reasoning_effort.and_then(non_empty_string),
    }
}

pub(crate) fn wrap_disabled_skills_policy(app: &AppHandle, prompt: &str) -> String {
    let disabled = disabled_skill_names(app);
    if disabled.is_empty() {
        return prompt.to_string();
    }
    format!(
        "## Reflex system skill policy\n\
The following Codex skills are disabled by Reflex system settings for this request: {}.\n\
Do not invoke these skills automatically and ignore project preferred-skill entries that match this disabled list unless the user explicitly re-enables them in Reflex settings.\n\n{}",
        disabled.join(", "),
        prompt
    )
}

pub(crate) fn request_kind_for_thread(plan_mode: bool, source: &str) -> RequestKind {
    if plan_mode {
        return RequestKind::Complex;
    }
    match source {
        "reflection" => RequestKind::Complex,
        "metadata" | "title" | "suggester" => RequestKind::Instant,
        _ => RequestKind::Fast,
    }
}

pub(crate) fn request_kind_from_params(params: &Value, default: RequestKind) -> RequestKind {
    let raw = params
        .get("request_profile")
        .or_else(|| params.get("requestProfile"))
        .or_else(|| params.get("profile"))
        .or_else(|| params.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "complex" | "deep" | "slow" => RequestKind::Complex,
        "instant" | "quick" | "light" => RequestKind::Instant,
        "fast" => RequestKind::Fast,
        _ => default,
    }
}

fn payload(app: &AppHandle) -> Result<SystemSettingsPayload, String> {
    let catalog = codex_catalog();
    let settings = read_settings(app)
        .map(|value| normalize_settings(value, &catalog.models, &catalog.defaults))
        .unwrap_or_else(|_| default_settings(&catalog.models, &catalog.defaults));
    write_settings(app, &settings)?;
    Ok(SystemSettingsPayload {
        settings,
        models: catalog.models,
        skills: catalog.skills,
        codex_home: catalog.codex_home.to_string_lossy().into_owned(),
        settings_path: settings_path(app)?.to_string_lossy().into_owned(),
        codex_model: catalog.defaults.model,
        codex_reasoning_effort: catalog.defaults.reasoning_effort,
    })
}

fn disabled_skill_names(app: &AppHandle) -> Vec<String> {
    let catalog = codex_catalog();
    let settings = read_settings(app)
        .map(|value| normalize_settings(value, &catalog.models, &catalog.defaults))
        .unwrap_or_else(|_| default_settings(&catalog.models, &catalog.defaults));
    settings.disabled_skills
}

fn read_settings(app: &AppHandle) -> Result<SystemSettings, String> {
    let path = settings_path(app)?;
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read settings: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse settings: {e}"))
}

fn write_settings(app: &AppHandle, settings: &SystemSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    std::fs::create_dir_all(&base).map_err(|e| format!("mkdir app_data_dir: {e}"))?;
    Ok(base.join(SETTINGS_FILE))
}

struct CodexCatalog {
    codex_home: PathBuf,
    defaults: CodexDefaults,
    models: Vec<CodexModel>,
    skills: Vec<CodexSkill>,
}

fn codex_catalog() -> CodexCatalog {
    let codex_home = codex_home();
    let defaults = read_codex_defaults(&codex_home);
    let models = read_models_cache(&codex_home);
    let skills = discover_skills(&codex_home, &defaults.enabled_plugins);
    CodexCatalog {
        codex_home,
        defaults,
        models,
        skills,
    }
}

fn codex_home() -> PathBuf {
    std::env::var("CODEX_HOME")
        .ok()
        .and_then(non_empty_string)
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn read_codex_defaults(codex_home: &Path) -> CodexDefaults {
    let mut defaults = CodexDefaults::default();
    let path = codex_home.join("config.toml");
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return defaults,
    };
    let mut current_plugin: Option<String> = None;
    let mut in_table = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_table = true;
            current_plugin = parse_plugin_table(trimmed);
            continue;
        }
        if !in_table {
            if let Some(value) = trimmed.strip_prefix("model =") {
                defaults.model = parse_toml_string(value);
            } else if let Some(value) = trimmed.strip_prefix("model_reasoning_effort =") {
                defaults.reasoning_effort = parse_toml_string(value);
            }
        }
        if let Some(plugin) = current_plugin.as_ref() {
            if trimmed == "enabled = true" {
                defaults.enabled_plugins.insert(plugin.clone());
            }
        }
    }
    defaults
}

fn parse_plugin_table(line: &str) -> Option<String> {
    let prefix = "[plugins.\"";
    let suffix = "\"]";
    if line.starts_with(prefix) && line.ends_with(suffix) {
        return Some(
            line.trim_start_matches(prefix)
                .trim_end_matches(suffix)
                .to_string(),
        );
    }
    None
}

fn parse_toml_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return non_empty_string(trimmed[1..trimmed.len() - 1].to_string());
    }
    non_empty_string(trimmed.to_string())
}

fn read_models_cache(codex_home: &Path) -> Vec<CodexModel> {
    let raw = match std::fs::read_to_string(codex_home.join("models_cache.json")) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return Vec::new(),
    };
    let mut models: Vec<CodexModel> = parsed
        .get("models")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(parse_model)
        .collect();
    models.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    models
}

fn parse_model(value: &Value) -> Option<CodexModel> {
    let slug = value
        .get("slug")
        .or_else(|| value.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|s| non_empty_string(s.to_string()))?;
    let display_name = value
        .get("display_name")
        .or_else(|| value.get("displayName"))
        .and_then(|v| v.as_str())
        .and_then(|s| non_empty_string(s.to_string()))
        .unwrap_or_else(|| slug.clone());
    let default_reasoning_level = value
        .get("default_reasoning_level")
        .or_else(|| value.get("defaultReasoningLevel"))
        .and_then(|v| v.as_str())
        .and_then(|s| non_empty_string(s.to_string()));
    let supported_reasoning_levels = value
        .get("supported_reasoning_levels")
        .or_else(|| value.get("supportedReasoningLevels"))
        .and_then(|v| v.as_array())
        .map(|levels| levels.iter().filter_map(parse_reasoning_level).collect())
        .unwrap_or_default();
    Some(CodexModel {
        slug,
        display_name,
        default_reasoning_level,
        supported_reasoning_levels,
    })
}

fn parse_reasoning_level(value: &Value) -> Option<CodexReasoningLevel> {
    if let Some(effort) = value.as_str().and_then(|s| non_empty_string(s.to_string())) {
        return Some(CodexReasoningLevel {
            label: effort.clone(),
            effort,
            description: String::new(),
        });
    }
    let effort = value
        .get("effort")
        .or_else(|| value.get("value"))
        .and_then(|v| v.as_str())
        .and_then(|s| non_empty_string(s.to_string()))?;
    let label = value
        .get("label")
        .and_then(|v| v.as_str())
        .and_then(|s| non_empty_string(s.to_string()))
        .unwrap_or_else(|| effort.clone());
    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(CodexReasoningLevel {
        effort,
        label,
        description,
    })
}

fn discover_skills(codex_home: &Path, enabled_plugins: &HashSet<String>) -> Vec<CodexSkill> {
    let mut skills = Vec::new();
    let mut seen = HashSet::new();
    let local_skills = codex_home.join("skills");
    collect_skill_files(&local_skills, &mut |path| {
        if path.components().any(|c| c.as_os_str() == ".tmp") {
            return;
        }
        if let Some(skill) = skill_from_path(path, None, "Codex skill") {
            push_skill(&mut skills, &mut seen, skill);
        }
    });
    collect_skill_files(&codex_home.join("plugins/cache"), &mut |path| {
        let plugin = plugin_id_for_skill(codex_home, path);
        if !enabled_plugins.is_empty() {
            if let Some(id) = plugin.as_ref() {
                if !enabled_plugins.contains(id) {
                    return;
                }
            }
        }
        let source = plugin
            .as_ref()
            .map(|id| format!("Codex plugin {id}"))
            .unwrap_or_else(|| "Codex plugin".into());
        if let Some(skill) = skill_from_path(path, plugin.as_deref(), &source) {
            push_skill(&mut skills, &mut seen, skill);
        }
    });
    if let Ok(home) = std::env::var("HOME") {
        collect_skill_files(&PathBuf::from(home).join(".agents/skills"), &mut |path| {
            if let Some(skill) = skill_from_path(path, None, "Agent skill") {
                push_skill(&mut skills, &mut seen, skill);
            }
        });
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn collect_skill_files(root: &Path, visitor: &mut impl FnMut(&Path)) {
    if !root.is_dir() {
        return;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|s| s.to_str()) == Some("SKILL.md") {
                visitor(&path);
            }
        }
    }
}

fn plugin_id_for_skill(codex_home: &Path, skill_path: &Path) -> Option<String> {
    let rel = skill_path
        .strip_prefix(codex_home.join("plugins/cache"))
        .ok()?;
    let parts: Vec<String> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
        .collect();
    if parts.len() < 2 {
        return None;
    }
    Some(format!("{}@{}", parts[1], parts[0]))
}

fn skill_from_path(path: &Path, plugin_id: Option<&str>, source: &str) -> Option<CodexSkill> {
    let raw = std::fs::read_to_string(path).ok()?;
    let fallback = path.parent()?.file_name()?.to_string_lossy().into_owned();
    let meta = parse_skill_frontmatter(&raw);
    let base_name = meta
        .get("name")
        .cloned()
        .and_then(non_empty_string)
        .unwrap_or(fallback);
    let name = if let Some(plugin_id) = plugin_id {
        let plugin_name = plugin_id.split('@').next().unwrap_or(plugin_id);
        format!("{plugin_name}:{base_name}")
    } else {
        base_name
    };
    let display_name = name.clone();
    let description = meta
        .get("description")
        .cloned()
        .or_else(|| first_non_heading_line(&raw))
        .unwrap_or_default();
    Some(CodexSkill {
        name,
        display_name,
        description,
        source: source.to_string(),
        path: path.to_string_lossy().into_owned(),
    })
}

fn parse_skill_frontmatter(raw: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some("---") {
        return out;
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let value = value.trim();
            if !value.is_empty() {
                out.insert(key.trim().to_string(), trim_quotes(value).to_string());
            }
        }
    }
    out
}

fn first_non_heading_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && *line != "---")
        .map(|s| s.to_string())
}

fn trim_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let first = value.as_bytes()[0] as char;
        let last = value.as_bytes()[value.len() - 1] as char;
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn push_skill(skills: &mut Vec<CodexSkill>, seen: &mut HashSet<String>, skill: CodexSkill) {
    if seen.insert(skill.name.to_ascii_lowercase()) {
        skills.push(skill);
    }
}

fn default_settings(models: &[CodexModel], defaults: &CodexDefaults) -> SystemSettings {
    let default_model = defaults
        .model
        .clone()
        .or_else(|| models.first().map(|m| m.slug.clone()));
    SystemSettings {
        version: settings_version(),
        request_profiles: RequestProfiles {
            complex: RequestProfile {
                model: default_model.clone(),
                reasoning_effort: Some(best_effort(
                    models,
                    default_model.as_deref(),
                    &["high", "xhigh", "medium", "low"],
                )),
            },
            fast: RequestProfile {
                model: default_model.clone(),
                reasoning_effort: Some(best_effort(
                    models,
                    default_model.as_deref(),
                    &["medium", "low", "high"],
                )),
            },
            instant: RequestProfile {
                model: default_model.clone(),
                reasoning_effort: Some(best_effort(
                    models,
                    default_model.as_deref(),
                    &["low", "minimal", "medium"],
                )),
            },
        },
        disabled_skills: Vec::new(),
    }
}

fn normalize_settings(
    mut settings: SystemSettings,
    models: &[CodexModel],
    defaults: &CodexDefaults,
) -> SystemSettings {
    let defaulted = default_settings(models, defaults);
    settings.version = settings_version();
    normalize_profile(
        &mut settings.request_profiles.complex,
        &defaulted.request_profiles.complex,
    );
    normalize_profile(
        &mut settings.request_profiles.fast,
        &defaulted.request_profiles.fast,
    );
    normalize_profile(
        &mut settings.request_profiles.instant,
        &defaulted.request_profiles.instant,
    );
    let mut seen = HashSet::new();
    settings.disabled_skills = settings
        .disabled_skills
        .into_iter()
        .filter_map(non_empty_string)
        .filter(|skill| seen.insert(skill.to_ascii_lowercase()))
        .collect();
    settings.disabled_skills.sort();
    settings
}

fn normalize_profile(profile: &mut RequestProfile, fallback: &RequestProfile) {
    profile.model = profile
        .model
        .take()
        .and_then(non_empty_string)
        .or_else(|| fallback.model.clone());
    profile.reasoning_effort = profile
        .reasoning_effort
        .take()
        .and_then(non_empty_string)
        .or_else(|| fallback.reasoning_effort.clone());
}

fn best_effort(models: &[CodexModel], model: Option<&str>, preferred: &[&str]) -> String {
    let supported = model
        .and_then(|slug| models.iter().find(|candidate| candidate.slug == slug))
        .map(|m| {
            m.supported_reasoning_levels
                .iter()
                .map(|level| level.effort.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for effort in preferred {
        if supported.is_empty() || supported.iter().any(|candidate| candidate == effort) {
            return (*effort).to_string();
        }
    }
    supported.first().copied().unwrap_or("medium").to_string()
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plugin_table_names() {
        assert_eq!(
            parse_plugin_table("[plugins.\"superpowers@openai-curated\"]"),
            Some("superpowers@openai-curated".into())
        );
        assert_eq!(parse_plugin_table("[projects.\"/tmp\"]"), None);
    }

    #[test]
    fn parses_skill_frontmatter() {
        let meta = parse_skill_frontmatter(
            "---\nname: brainstorming\ndescription: \"Explore ideas\"\n---\n# Title\n",
        );
        assert_eq!(meta.get("name").map(String::as_str), Some("brainstorming"));
        assert_eq!(
            meta.get("description").map(String::as_str),
            Some("Explore ideas")
        );
    }

    #[test]
    fn request_kind_respects_plan_mode() {
        assert!(matches!(
            request_kind_for_thread(true, "quick"),
            RequestKind::Complex
        ));
        assert!(matches!(
            request_kind_for_thread(false, "metadata"),
            RequestKind::Instant
        ));
        assert!(matches!(
            request_kind_for_thread(false, "quick"),
            RequestKind::Fast
        ));
    }
}
