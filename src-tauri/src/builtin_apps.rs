use crate::apps::{self, AppManifest};
use std::fs;
use std::io;
use tauri::AppHandle;

struct BuiltinApp {
    id: &'static str,
    manifest: &'static str,
    files: &'static [(&'static str, &'static str)],
}

pub fn ensure(app: &AppHandle) -> io::Result<()> {
    for built in BUILTIN_APPS {
        ensure_one(app, built)?;
    }
    apps::ensure_system_app_folder(app)?;
    Ok(())
}

fn ensure_one(app: &AppHandle, built: &BuiltinApp) -> io::Result<()> {
    let dir = apps::app_dir(app, built.id)?;
    if dir.join("manifest.json").is_file() {
        return Ok(());
    }
    let mut manifest: AppManifest = serde_json::from_str(built.manifest)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    if manifest.id != built.id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("builtin app id mismatch: {} != {}", manifest.id, built.id),
        ));
    }
    manifest.folder_path = Some(apps::SYSTEM_APP_FOLDER.into());

    fs::create_dir_all(&dir)?;
    apps::write_manifest(app, built.id, &manifest)?;
    for (path, contents) in built.files {
        let rel = safe_relative_path(path)?;
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, contents)?;
    }
    let meta_dir = dir.join(".reflex");
    fs::create_dir_all(&meta_dir)?;
    fs::write(
        meta_dir.join("builtin.json"),
        serde_json::json!({
            "id": built.id,
            "installed_by": "reflex",
            "version": 1,
        })
        .to_string(),
    )?;
    let _ = apps::git_commit_all(&dir, "install built-in utility");
    Ok(())
}

fn safe_relative_path(path: &str) -> io::Result<&str> {
    let rel = std::path::Path::new(path);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsafe builtin path: {path}"),
        ));
    }
    Ok(path)
}

const BUILTIN_APPS: &[BuiltinApp] = &[
    BuiltinApp {
        id: "system-quick-capture",
        manifest: include_str!("../builtin_apps/system-quick-capture/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-quick-capture/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-project-cockpit",
        manifest: include_str!("../builtin_apps/system-project-cockpit/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-project-cockpit/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-research-capture",
        manifest: include_str!("../builtin_apps/system-research-capture/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-research-capture/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-bridge-console",
        manifest: include_str!("../builtin_apps/system-bridge-console/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-bridge-console/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-automation-center",
        manifest: include_str!("../builtin_apps/system-automation-center/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-automation-center/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-clipboard-snippets",
        manifest: include_str!("../builtin_apps/system-clipboard-snippets/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-clipboard-snippets/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-project-file-lens",
        manifest: include_str!("../builtin_apps/system-project-file-lens/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-project-file-lens/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-memory-capsule",
        manifest: include_str!("../builtin_apps/system-memory-capsule/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-memory-capsule/index.html"),
        )],
    },
    BuiltinApp {
        id: "system-api-workbench",
        manifest: include_str!("../builtin_apps/system-api-workbench/manifest.json"),
        files: &[(
            "index.html",
            include_str!("../builtin_apps/system-api-workbench/index.html"),
        )],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_manifests_are_valid_and_use_safe_assets() {
        assert!(BUILTIN_APPS.len() >= 6);
        for built in BUILTIN_APPS {
            let manifest: AppManifest = serde_json::from_str(built.manifest).expect(built.id);
            assert_eq!(manifest.id, built.id);
            assert!(!manifest.name.trim().is_empty());
            assert!(manifest.entry.ends_with(".html"));
            assert!(!manifest.actions.is_empty(), "{}", built.id);
            assert!(!manifest.widgets.is_empty(), "{}", built.id);
            for (path, contents) in built.files {
                safe_relative_path(path).expect(path);
                assert!(!contents.trim().is_empty(), "{}", path);
                assert!(!contents.contains("localStorage"), "{}", path);
                assert!(!contents.contains("sessionStorage"), "{}", path);
            }
        }
    }
}
