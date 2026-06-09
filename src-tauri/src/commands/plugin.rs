use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::plugin_loader::{PluginDetail, PluginSummary};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginImportItem {
    pub name: String,
    pub version: Option<String>,
    pub source: String,
    pub destination: String,
    pub updated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginImportError {
    pub source: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginImportResult {
    pub source_dir: String,
    pub destination_dir: String,
    pub imported: Vec<PluginImportItem>,
    pub errors: Vec<PluginImportError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUninstallResult {
    pub plugin_id: String,
    pub removed_path: String,
}

#[tauri::command]
pub async fn plugin_list(state: State<'_, AppState>) -> AppResult<Vec<PluginSummary>> {
    Ok(crate::plugin_loader::list_plugins(
        &state.workspace_config_dir,
    ))
}

#[tauri::command]
pub async fn plugin_read(state: State<'_, AppState>, plugin_id: String) -> AppResult<PluginDetail> {
    crate::plugin_loader::read_plugin(&state.workspace_config_dir, &plugin_id)
        .ok_or_else(|| AppError::Custom(format!("Plugin not found: {plugin_id}")))
}

#[tauri::command]
pub async fn plugin_set_enabled(
    state: State<'_, AppState>,
    plugin_id: String,
    enabled: bool,
) -> AppResult<PluginSummary> {
    crate::plugin_loader::set_plugin_enabled(&state.workspace_config_dir, &plugin_id, enabled)
        .map_err(AppError::Custom)
}

#[tauri::command]
pub async fn plugin_uninstall(
    state: State<'_, AppState>,
    plugin_id: String,
) -> AppResult<PluginUninstallResult> {
    let plugin_path = state.workspace_config_dir.join("plugins").join(&plugin_id);
    crate::plugin_loader::uninstall_plugin(&state.workspace_config_dir, &plugin_id)
        .map_err(AppError::Custom)?;
    Ok(PluginUninstallResult {
        plugin_id,
        removed_path: plugin_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub async fn plugin_import_codex_cache(
    state: State<'_, AppState>,
    source_dir: Option<String>,
) -> AppResult<PluginImportResult> {
    let source_dir = match source_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => PathBuf::from(value),
        None => default_codex_plugin_cache_dir()
            .ok_or_else(|| AppError::Custom("Could not locate the Codex plugin cache".into()))?,
    };
    import_codex_plugin_cache(&source_dir, &state.workspace_config_dir.join("plugins"))
}

pub(crate) fn default_codex_plugin_cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    Some(home.join(".codex").join("plugins").join("cache"))
}

fn import_codex_plugin_cache(
    source_dir: &Path,
    destination_dir: &Path,
) -> AppResult<PluginImportResult> {
    if !source_dir.is_dir() {
        return Err(AppError::Custom(format!(
            "Codex plugin cache not found: {}",
            source_dir.display()
        )));
    }

    fs::create_dir_all(destination_dir)?;
    let mut roots = discover_plugin_roots(source_dir);
    roots.sort();
    roots.dedup();

    let mut used_destination_names = BTreeSet::new();
    let mut imported = Vec::new();
    let mut errors = Vec::new();

    for root in roots {
        match import_single_plugin_root(&root, destination_dir, &mut used_destination_names) {
            Ok(item) => imported.push(item),
            Err(error) => errors.push(PluginImportError {
                source: root.to_string_lossy().to_string(),
                error,
            }),
        }
    }

    imported.sort_by(|left, right| left.name.cmp(&right.name));
    errors.sort_by(|left, right| left.source.cmp(&right.source));

    Ok(PluginImportResult {
        source_dir: source_dir.to_string_lossy().to_string(),
        destination_dir: destination_dir.to_string_lossy().to_string(),
        imported,
        errors,
    })
}

pub(crate) fn discover_plugin_roots(source_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    discover_plugin_roots_inner(source_dir, &mut roots);
    roots
}

fn discover_plugin_roots_inner(dir: &Path, roots: &mut Vec<PathBuf>) {
    let manifest = dir.join(".codex-plugin").join("plugin.json");
    if manifest.is_file() {
        roots.push(dir.to_path_buf());
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && !file_type.is_symlink() {
            discover_plugin_roots_inner(&path, roots);
        }
    }
}

fn import_single_plugin_root(
    root: &Path,
    destination_dir: &Path,
    used_destination_names: &mut BTreeSet<String>,
) -> Result<PluginImportItem, String> {
    let manifest_path = root.join(".codex-plugin").join("plugin.json");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("failed to read manifest: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|error| format!("failed to parse manifest: {error}"))?;
    let name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "manifest is missing a non-empty name".to_string())?
        .to_string();
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let destination_name = unique_plugin_destination_name(&name, used_destination_names);
    let destination = destination_dir.join(destination_name);
    let updated = destination.exists();
    copy_plugin_dir(root, &destination)
        .map_err(|error| format!("failed to copy plugin files: {error}"))?;

    Ok(PluginImportItem {
        name,
        version,
        source: root.to_string_lossy().to_string(),
        destination: destination.to_string_lossy().to_string(),
        updated,
    })
}

pub(crate) fn import_plugin_root(
    root: &Path,
    destination_dir: &Path,
) -> Result<PluginImportItem, String> {
    let mut used_destination_names = BTreeSet::new();
    import_single_plugin_root(root, destination_dir, &mut used_destination_names)
}

fn unique_plugin_destination_name(name: &str, used_names: &mut BTreeSet<String>) -> String {
    let base = sanitize_plugin_destination_name(name);
    let mut candidate = base.clone();
    let mut counter = 2usize;
    while used_names.contains(&candidate) {
        candidate = format!("{base}-{counter}");
        counter += 1;
    }
    used_names.insert(candidate.clone());
    candidate
}

pub(crate) fn sanitize_plugin_destination_name(name: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            output.push('-');
            last_dash = true;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "plugin".to_string()
    } else {
        trimmed
    }
}

fn copy_plugin_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = destination.join(entry.file_name());

        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            copy_plugin_dir(&from, &to)?;
        } else if file_type.is_file() {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_codex_plugin_cache_copies_plugin_roots() {
        let root = std::env::temp_dir().join(format!(
            "cn-codex-plugin-import-test-{}",
            uuid::Uuid::new_v4()
        ));
        let cache = root.join("cache");
        let plugin_root = cache.join("openai-bundled").join("browser").join("1.0.0");
        write_file(
            &plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"Browser","version":"1.0.0","skills":"./skills"}"#,
        );
        write_file(
            &plugin_root.join("skills").join("control").join("SKILL.md"),
            "# Control browser\n",
        );

        let destination = root.join("codey").join("plugins");
        let result = import_codex_plugin_cache(&cache, &destination).unwrap();

        assert_eq!(result.imported.len(), 1);
        assert_eq!(result.imported[0].name, "Browser");
        assert!(!result.imported[0].updated);
        assert!(
            destination
                .join("browser/.codex-plugin/plugin.json")
                .is_file()
        );
        assert!(
            destination
                .join("browser/skills/control/SKILL.md")
                .is_file()
        );

        let second = import_codex_plugin_cache(&cache, &destination).unwrap();
        assert_eq!(second.imported.len(), 1);
        assert!(second.imported[0].updated);

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn sanitize_plugin_destination_name_is_stable() {
        assert_eq!(sanitize_plugin_destination_name("Browser"), "browser");
        assert_eq!(
            sanitize_plugin_destination_name("OpenAI Primary Runtime"),
            "openai-primary-runtime"
        );
        assert_eq!(sanitize_plugin_destination_name("!!!"), "plugin");
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }
}
