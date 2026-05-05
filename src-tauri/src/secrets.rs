//! Reflex OS secrets store.
//!
//! Two scopes: `global` (one shared file) and `project:<id>` (one file per
//! linked Reflex project). Values are encrypted at rest with AES-GCM. The
//! 32-byte master key is generated on first use and stored in macOS
//! Keychain via the `keyring` crate; only the encrypted blob and a 12-byte
//! nonce are written to disk.
//!
//! On non-macOS hosts, falls back to a master key file with `0o600` perms in
//! the app data dir. Tauri ships only macOS today, so this fallback is mostly
//! to keep `cargo check` portable.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

const KEYCHAIN_SERVICE: &str = "com.reflex.os.secrets";
const KEYCHAIN_ACCOUNT: &str = "master-key";
const SECRETS_DIRNAME: &str = "secrets";
const FILE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope<'a> {
    Global,
    Project(&'a str),
}

impl Scope<'_> {
    fn label(&self) -> String {
        match self {
            Scope::Global => "global".into(),
            Scope::Project(id) => format!("project:{id}"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SecretEntry {
    pub value: String,
    #[serde(default)]
    pub updated_at_ms: u128,
    #[serde(default)]
    pub source_app_id: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct SecretMetadata {
    pub key: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub updated_at_ms: u128,
    pub source_app_id: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct SecretsPayload {
    #[serde(default)]
    entries: BTreeMap<String, SecretEntry>,
}

#[derive(Serialize, Deserialize)]
struct EncryptedFile {
    version: u32,
    nonce_b64: String,
    ciphertext_b64: String,
}

static MASTER_KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn secrets_root(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    let dir = base.join(SECRETS_DIRNAME);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir secrets dir: {e}"))?;
    set_dir_mode_700(&dir);
    Ok(dir)
}

fn scope_path(app: &AppHandle, scope: Scope<'_>) -> Result<PathBuf, String> {
    let root = secrets_root(app)?;
    let filename = match scope {
        Scope::Global => "global.json.enc".to_string(),
        Scope::Project(id) => format!("project-{}.json.enc", sanitize_id(id)),
    };
    Ok(root.join(filename))
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(unix)]
fn set_dir_mode_700(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.mode() & 0o777 != 0o700 {
            perms.set_mode(0o700);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

#[cfg(not(unix))]
fn set_dir_mode_700(_path: &Path) {}

#[cfg(unix)]
fn set_file_mode_600(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_file_mode_600(_path: &Path) {}

// ---------------------------------------------------------------------------
// Master key acquisition
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn load_master_key(_app: &AppHandle) -> Result<[u8; 32], String> {
    if let Some(cached) = *MASTER_KEY.lock().expect("master key mutex poisoned") {
        return Ok(cached);
    }
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|e| format!("keyring entry: {e}"))?;
    let key = match entry.get_password() {
        Ok(b64) => decode_master(&b64)?,
        Err(keyring::Error::NoEntry) => {
            let fresh = generate_master();
            let encoded = BASE64.encode(fresh);
            entry
                .set_password(&encoded)
                .map_err(|e| format!("keyring set: {e}"))?;
            fresh
        }
        Err(err) => return Err(format!("keyring get: {err}")),
    };
    *MASTER_KEY.lock().expect("master key mutex poisoned") = Some(key);
    Ok(key)
}

#[cfg(not(target_os = "macos"))]
fn load_master_key(app: &AppHandle) -> Result<[u8; 32], String> {
    if let Some(cached) = *MASTER_KEY.lock().expect("master key mutex poisoned") {
        return Ok(cached);
    }
    let path = secrets_root(app)?.join("master.key");
    let key = if path.exists() {
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("read master: {e}"))?;
        decode_master(raw.trim())?
    } else {
        let fresh = generate_master();
        std::fs::write(&path, BASE64.encode(fresh)).map_err(|e| format!("write master: {e}"))?;
        set_file_mode_600(&path);
        fresh
    };
    *MASTER_KEY.lock().expect("master key mutex poisoned") = Some(key);
    Ok(key)
}

fn generate_master() -> [u8; 32] {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

fn decode_master(b64: &str) -> Result<[u8; 32], String> {
    let bytes = BASE64.decode(b64).map_err(|e| format!("master key b64: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("master key wrong length: {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Read / write payloads
// ---------------------------------------------------------------------------

fn read_payload(app: &AppHandle, scope: Scope<'_>) -> Result<SecretsPayload, String> {
    let path = scope_path(app, scope)?;
    if !path.exists() {
        return Ok(SecretsPayload::default());
    }
    let raw = std::fs::read(&path).map_err(|e| format!("read secrets file: {e}"))?;
    if raw.is_empty() {
        return Ok(SecretsPayload::default());
    }
    let envelope: EncryptedFile =
        serde_json::from_slice(&raw).map_err(|e| format!("parse secrets envelope: {e}"))?;
    if envelope.version != FILE_VERSION {
        return Err(format!(
            "unsupported secrets file version {} for scope {}",
            envelope.version,
            scope.label()
        ));
    }
    let nonce_bytes = BASE64
        .decode(envelope.nonce_b64.as_bytes())
        .map_err(|e| format!("nonce b64: {e}"))?;
    let ciphertext = BASE64
        .decode(envelope.ciphertext_b64.as_bytes())
        .map_err(|e| format!("ciphertext b64: {e}"))?;
    if nonce_bytes.len() != 12 {
        return Err("nonce must be 12 bytes".into());
    }
    let key_bytes = load_master_key(app)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "secrets decrypt failed (master key changed?)".to_string())?;
    let payload: SecretsPayload =
        serde_json::from_slice(&plaintext).map_err(|e| format!("parse secrets payload: {e}"))?;
    Ok(payload)
}

fn write_payload(
    app: &AppHandle,
    scope: Scope<'_>,
    payload: &SecretsPayload,
) -> Result<(), String> {
    let path = scope_path(app, scope)?;
    let key_bytes = load_master_key(app)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = serde_json::to_vec(payload).map_err(|e| format!("serialize payload: {e}"))?;
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| format!("encrypt: {e}"))?;
    let envelope = EncryptedFile {
        version: FILE_VERSION,
        nonce_b64: BASE64.encode(nonce_bytes),
        ciphertext_b64: BASE64.encode(ciphertext),
    };
    let body = serde_json::to_vec_pretty(&envelope).map_err(|e| format!("serialize envelope: {e}"))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &body).map_err(|e| format!("write tmp: {e}"))?;
    set_file_mode_600(&tmp);
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename tmp: {e}"))?;
    set_file_mode_600(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Public CRUD API
// ---------------------------------------------------------------------------

pub fn list(app: &AppHandle, scope: Scope<'_>) -> Result<Vec<SecretMetadata>, String> {
    let payload = read_payload(app, scope)?;
    let project_id = match scope {
        Scope::Global => None,
        Scope::Project(id) => Some(id.to_string()),
    };
    let scope_label = match scope {
        Scope::Global => "global".to_string(),
        Scope::Project(_) => "project".to_string(),
    };
    let mut out: Vec<SecretMetadata> = payload
        .entries
        .into_iter()
        .map(|(key, entry)| SecretMetadata {
            key,
            scope: scope_label.clone(),
            project_id: project_id.clone(),
            updated_at_ms: entry.updated_at_ms,
            source_app_id: entry.source_app_id,
        })
        .collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

pub fn get(app: &AppHandle, scope: Scope<'_>, key: &str) -> Result<Option<SecretEntry>, String> {
    let payload = read_payload(app, scope)?;
    Ok(payload.entries.get(key).cloned())
}

pub fn has(app: &AppHandle, scope: Scope<'_>, key: &str) -> Result<bool, String> {
    let payload = read_payload(app, scope)?;
    Ok(payload.entries.contains_key(key))
}

pub fn set(
    app: &AppHandle,
    scope: Scope<'_>,
    key: &str,
    value: &str,
    source_app_id: &str,
) -> Result<SecretMetadata, String> {
    if key.trim().is_empty() {
        return Err("secret key must be non-empty".into());
    }
    let mut payload = match read_payload(app, scope) {
        Ok(p) => p,
        Err(err) if err.contains("decrypt failed") => return Err(err),
        Err(_) => SecretsPayload::default(),
    };
    let entry = SecretEntry {
        value: value.to_string(),
        updated_at_ms: now_ms(),
        source_app_id: Some(source_app_id.to_string()),
    };
    payload.entries.insert(key.to_string(), entry.clone());
    write_payload(app, scope, &payload)?;
    Ok(SecretMetadata {
        key: key.to_string(),
        scope: match scope {
            Scope::Global => "global".to_string(),
            Scope::Project(_) => "project".to_string(),
        },
        project_id: match scope {
            Scope::Global => None,
            Scope::Project(id) => Some(id.to_string()),
        },
        updated_at_ms: entry.updated_at_ms,
        source_app_id: entry.source_app_id,
    })
}

pub fn delete(app: &AppHandle, scope: Scope<'_>, key: &str) -> Result<bool, String> {
    let mut payload = read_payload(app, scope)?;
    let removed = payload.entries.remove(key).is_some();
    if removed {
        write_payload(app, scope, &payload)?;
    }
    Ok(removed)
}

/// Cascade lookup: for each project id (in caller-supplied order), then
/// global. Returns the first match plus its origin so callers can render
/// "from project X" UI.
pub fn resolve(
    app: &AppHandle,
    project_ids: &[String],
    key: &str,
) -> Result<Option<(SecretMetadata, SecretEntry)>, String> {
    for project_id in project_ids {
        if let Some(entry) = get(app, Scope::Project(project_id), key)? {
            let meta = SecretMetadata {
                key: key.to_string(),
                scope: "project".to_string(),
                project_id: Some(project_id.clone()),
                updated_at_ms: entry.updated_at_ms,
                source_app_id: entry.source_app_id.clone(),
            };
            return Ok(Some((meta, entry)));
        }
    }
    if let Some(entry) = get(app, Scope::Global, key)? {
        let meta = SecretMetadata {
            key: key.to_string(),
            scope: "global".to_string(),
            project_id: None,
            updated_at_ms: entry.updated_at_ms,
            source_app_id: entry.source_app_id.clone(),
        };
        return Ok(Some((meta, entry)));
    }
    Ok(None)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn destroy_master_key_cache() {
    *MASTER_KEY.lock().expect("master key mutex poisoned") = None;
}

// io::Error shim kept for the few callers that want a stable error type.
#[allow(dead_code)]
fn other_io(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, message.into())
}
