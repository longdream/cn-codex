//! Tauri commands for MiniApp registry and runtime.

use serde::Deserialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::runtime::{self, start_app, stop_app};
use super::scaffold::{scaffold_miniapp, ScaffoldRequest};
use super::{
    find_app, find_app_mut, load_registry, open_page_url, save_registry, validate_slug, write_manifest,
    MiniAppRecord, MiniAppStatus,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniAppCreateArgs {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub database_id: String,
    #[serde(default)]
    pub database_name: String,
}

#[tauri::command]
pub async fn miniapp_list(state: State<'_, AppState>) -> AppResult<Vec<MiniAppRecord>> {
    let dir = state.workspace_config_dir.clone();
    let apps = load_registry(&dir).map_err(AppError::Custom)?;
    Ok(apps)
}

#[tauri::command]
pub async fn miniapp_create(
    state: State<'_, AppState>,
    args: MiniAppCreateArgs,
) -> AppResult<MiniAppRecord> {
    let dir = state.workspace_config_dir.clone();
    let name = args.name.trim().to_string();
    let slug = args.slug.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err(AppError::Custom("小程序名称不能为空".into()));
    }
    validate_slug(&slug).map_err(AppError::Custom)?;

    let mut apps = load_registry(&dir).map_err(AppError::Custom)?;
    if apps.iter().any(|a| a.slug == slug) {
        return Err(AppError::Custom(format!("英文名称 `{slug}` 已存在")));
    }

    let record = scaffold_miniapp(
        &dir,
        ScaffoldRequest {
            name,
            slug,
            description: args.description.trim().to_string(),
            database_id: args.database_id.trim().to_string(),
            database_name: args.database_name.trim().to_string(),
        },
    )
    .map_err(AppError::Custom)?;

    apps.insert(0, record.clone());
    save_registry(&dir, &apps).map_err(AppError::Custom)?;
    Ok(record)
}

#[tauri::command]
pub async fn miniapp_start(state: State<'_, AppState>, id_or_slug: String) -> AppResult<MiniAppRecord> {
    let dir = state.workspace_config_dir.clone();
    let mut apps = load_registry(&dir).map_err(AppError::Custom)?;
    let Some(idx) = apps
        .iter()
        .position(|a| a.id == id_or_slug || a.slug == id_or_slug)
    else {
        return Err(AppError::Custom("小程序不存在".into()));
    };
    let mut app = apps[idx].clone();
    match start_app(&dir, &mut app) {
        Ok(_) => {
            apps[idx] = app.clone();
            save_registry(&dir, &apps).map_err(AppError::Custom)?;
            Ok(app)
        }
        Err(err) => {
            app.status = MiniAppStatus::Error;
            app.last_error = err.clone();
            let _ = write_manifest(&app);
            apps[idx] = app;
            let _ = save_registry(&dir, &apps);
            Err(AppError::Custom(err))
        }
    }
}

#[tauri::command]
pub async fn miniapp_stop(state: State<'_, AppState>, id_or_slug: String) -> AppResult<MiniAppRecord> {
    let dir = state.workspace_config_dir.clone();
    let mut apps = load_registry(&dir).map_err(AppError::Custom)?;
    let Some(app) = find_app_mut(&mut apps, &id_or_slug) else {
        return Err(AppError::Custom("小程序不存在".into()));
    };
    stop_app(app).map_err(AppError::Custom)?;
    let result = app.clone();
    save_registry(&dir, &apps).map_err(AppError::Custom)?;
    Ok(result)
}

#[tauri::command]
pub async fn miniapp_delete(state: State<'_, AppState>, id_or_slug: String) -> AppResult<()> {
    let dir = state.workspace_config_dir.clone();
    let mut apps = load_registry(&dir).map_err(AppError::Custom)?;
    let Some(idx) = apps
        .iter()
        .position(|a| a.id == id_or_slug || a.slug == id_or_slug)
    else {
        return Err(AppError::Custom("小程序不存在".into()));
    };
    let mut app = apps.remove(idx);
    let _ = stop_app(&mut app);
    if !app.root_path.trim().is_empty() {
        let root = std::path::PathBuf::from(&app.root_path);
        if root.exists() {
            let _ = std::fs::remove_dir_all(&root);
        }
    }
    save_registry(&dir, &apps).map_err(AppError::Custom)?;
    Ok(())
}

#[tauri::command]
pub async fn miniapp_open_page(
    state: State<'_, AppState>,
    id_or_slug: String,
    page_id: Option<String>,
) -> AppResult<serde_json::Value> {
    let dir = state.workspace_config_dir.clone();
    let mut apps = load_registry(&dir).map_err(AppError::Custom)?;
    let Some(idx) = apps
        .iter()
        .position(|a| a.id == id_or_slug || a.slug == id_or_slug)
    else {
        return Err(AppError::Custom("小程序不存在".into()));
    };

    if !runtime::is_running(&apps[idx].slug) {
        let mut app = apps[idx].clone();
        start_app(&dir, &mut app).map_err(AppError::Custom)?;
        apps[idx] = app;
        save_registry(&dir, &apps).map_err(AppError::Custom)?;
    }

    let app = find_app(&apps, &id_or_slug).ok_or_else(|| AppError::Custom("小程序不存在".into()))?;
    let url = open_page_url(app, page_id.as_deref())
        .ok_or_else(|| AppError::Custom("小程序未分配端口，无法打开页面".into()))?;
    Ok(serde_json::json!({
        "ok": true,
        "url": url,
        "slug": app.slug,
        "name": app.name,
        "port": app.port,
        "pageId": page_id,
    }))
}
