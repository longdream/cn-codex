//! 语音模型下载管理器。
//!
//! 功能：
//! - 启动时自动检查并后台自动下载缺失模型（需求 3.1）；
//! - 断点续传（HTTP Range）；
//! - ModelScope 主源失败自动切换 GitHub 备源；
//! - 进度通过 Tauri 事件 `voice://download-progress` 推送前端（右下角下载列表）；
//! - tar.bz2 打包自动解压；目录清单模式批量下载。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::models::{
    self, ModelScopeFileEntry, ModelScopeFileList, VoiceModelDef, VoiceModelsManifest,
};

const EVENT_PROGRESS: &str = "voice://download-progress";
const CHUNK: usize = 256 * 1024;
/// 进度事件节流间隔（毫秒）。
const PROGRESS_INTERVAL_MS: u64 = 300;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model_id: String,
    pub label: String,
    pub status: String, // downloading / extracting / ready / failed
    pub downloaded: u64,
    pub total: u64,
    /// 字节/秒，-1 表示未知。
    pub speed: i64,
    pub error: Option<String>,
}

pub struct VoiceDownloadManager {
    models_dir: PathBuf,
    /// 正在下载中的模型（避免重复触发）。
    active: Mutex<HashMap<String, ()>>,
}

impl VoiceDownloadManager {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            active: Mutex::new(HashMap::new()),
        }
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// 启动检查：返回缺失需下载的模型列表。
    pub fn find_missing(&self, defs: &[VoiceModelDef]) -> Vec<VoiceModelDef> {
        let manifest = VoiceModelsManifest::load(&self.models_dir);
        defs.iter()
            .filter(|def| {
                let marker_ready = models::is_model_ready(&self.models_dir, def);
                let manifest_ready = manifest
                    .models
                    .get(&def.id)
                    .map(|s| s.status == "ready")
                    .unwrap_or(false);
                !(marker_ready && manifest_ready)
            })
            .cloned()
            .collect()
    }

    /// 下载指定模型（幂等：同模型并发请求被忽略）。
    pub async fn download_model(
        &self,
        app: tauri::AppHandle,
        def: VoiceModelDef,
    ) -> Result<(), String> {
        {
            let mut active = self.active.lock().await;
            if active.contains_key(&def.id) {
                return Ok(());
            }
            active.insert(def.id.clone(), ());
        }

        let result = self.download_inner(&app, &def).await;

        {
            let mut active = self.active.lock().await;
            active.remove(&def.id);
        }

        match &result {
            Ok(()) => {
                let mut manifest = VoiceModelsManifest::load(&self.models_dir);
                manifest.set_status(&def.id, "ready", None);
                let _ = manifest.save(&self.models_dir);
                let _ = self.emit(&app, &def, "ready", def.total_size, def.total_size, -1, None);
                info!("[voice] model {} downloaded and ready", def.id);
            }
            Err(err) => {
                warn!("[voice] model {} download failed: {err}", def.id);
                let mut manifest = VoiceModelsManifest::load(&self.models_dir);
                manifest.set_status(&def.id, "failed", None);
                let _ = manifest.save(&self.models_dir);
                let _ = self.emit(&app, &def, "failed", 0, def.total_size, -1, Some(err.clone()));
            }
        }
        result
    }

    async fn emit(
        &self,
        app: &tauri::AppHandle,
        def: &VoiceModelDef,
        status: &str,
        downloaded: u64,
        total: u64,
        speed: i64,
        error: Option<String>,
    ) -> Result<(), String> {
        use tauri::Emitter;
        let payload = DownloadProgress {
            model_id: def.id.clone(),
            label: def.label.clone(),
            status: status.to_string(),
            downloaded,
            total,
            speed,
            error,
        };
        app.emit(EVENT_PROGRESS, &payload).map_err(|e| e.to_string())
    }

    async fn download_inner(&self, app: &tauri::AppHandle, def: &VoiceModelDef) -> Result<(), String> {
        let target_dir = self.models_dir.join(&def.dir);
        tokio::fs::create_dir_all(&target_dir)
            .await
            .map_err(|e| format!("create model dir failed: {e}"))?;

        // 依序尝试各下载源。
        let mut last_err = String::new();
        for source in &def.sources {
            // ModelScope 主源：归档直链 + manifest 目录模式。
            let ms_result = match def.kind.as_str() {
                "archive" => {
                    self.download_archive(app, def, &source.modelscope, &target_dir, true)
                        .await
                }
                "single" => {
                    let file_name = source
                        .modelscope
                        .rsplit('/')
                        .next()
                        .unwrap_or("model.onnx")
                        .to_string();
                    self.download_single_file(
                        app,
                        def,
                        &source.modelscope,
                        &target_dir.join(file_name),
                        true,
                    )
                    .await
                }
                "manifest" => {
                    self.download_manifest_model(app, def, source, &target_dir, true)
                        .await
                }
                other => Err(format!("unknown model kind: {other}")),
            };
            if ms_result.is_ok() {
                return Ok(());
            }
            last_err = ms_result.unwrap_err();
            warn!("[voice] modelscope source failed for {}: {last_err}", def.id);

            // GitHub 备源。
            if let Some(github_url) = &source.github {
                let gh_result = match def.kind.as_str() {
                    "archive" => {
                        self.download_archive(app, def, github_url, &target_dir, false)
                            .await
                    }
                    "single" => {
                        let file_name = github_url
                            .rsplit('/')
                            .next()
                            .unwrap_or("model.onnx")
                            .to_string();
                        self.download_single_file(
                            app,
                            def,
                            github_url,
                            &target_dir.join(file_name),
                            false,
                        )
                        .await
                    }
                    "manifest" => {
                        // manifest 模式备源是打包文件，直接下载解压。
                        self.download_archive(app, def, github_url, &target_dir, false)
                            .await
                    }
                    _ => unreachable!(),
                };
                if gh_result.is_ok() {
                    return Ok(());
                }
                last_err = gh_result.unwrap_err();
                warn!("[voice] github source failed for {}: {last_err}", def.id);
            }
        }
        Err(last_err)
    }

    /// 下载 tar.bz2 归档并解压到目标目录。
    async fn download_archive(
        &self,
        app: &tauri::AppHandle,
        def: &VoiceModelDef,
        url: &str,
        target_dir: &Path,
        is_modelscope: bool,
    ) -> Result<(), String> {
        let archive_path = self
            .download_to_file(app, def, url, &format!("{}.tar.bz2", def.id), is_modelscope)
            .await?;

        self.emit(app, def, "extracting", def.total_size, def.total_size, -1, None)
            .await
            .ok();

        // 解包放到 blocking 线程，避免阻塞 runtime。
        let archive = archive_path.clone();
        let target = target_dir.to_path_buf();
        let extract_result = tauri::async_runtime::spawn_blocking(move || {
            extract_tar_bz2(&archive, &target)
        })
        .await
        .map_err(|e| format!("extract task join failed: {e}"))?;

        extract_result?;
        let _ = tokio::fs::remove_file(&archive_path).await;
        Ok(())
    }

    /// 下载单文件（VAD）。
    async fn download_single_file(
        &self,
        app: &tauri::AppHandle,
        def: &VoiceModelDef,
        url: &str,
        target: &Path,
        is_modelscope: bool,
    ) -> Result<(), String> {
        self.download_to_file(app, def, url, target, is_modelscope)
            .await
            .map(|_| ())
    }

    /// manifest 目录模式：拉取 ModelScope 文件清单，批量下载所有文件。
    async fn download_manifest_model(
        &self,
        app: &tauri::AppHandle,
        def: &VoiceModelDef,
        source: &models::VoiceModelSource,
        target_dir: &Path,
        is_modelscope: bool,
    ) -> Result<(), String> {
        if !is_modelscope {
            return Err("manifest mode only supports modelscope".to_string());
        }
        let base = source.modelscope.trim_end_matches('/');
        let api_url = format!("{base}/repo/files?Recursive=true");
        let http = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 cn-codex")
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let resp = http
            .get(&api_url)
            .send()
            .await
            .map_err(|e| format!("file list request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("file list status {}", resp.status()));
        }
        let list: ModelScopeFileList = resp
            .json()
            .await
            .map_err(|e| format!("file list parse failed: {e}"))?;
        let files = list
            .data
            .and_then(|d| d.files)
            .ok_or("file list empty")?;

        // 过滤出本模型需要的文件：注册的前缀目录 + 具体文件。
        let prefixes: Vec<String> = def
            .files
            .iter()
            .filter(|f| def_is_dir(&files, f))
            .cloned()
            .collect();
        let wanted: Vec<&ModelScopeFileEntry> = files
            .iter()
            .filter(|entry| {
                if entry.entry_type != "blob" && entry.size == 0 {
                    return false;
                }
                for f in &def.files {
                    if &entry.path == f {
                        return true;
                    }
                }
                for p in &prefixes {
                    if entry.path.starts_with(&format!("{p}/")) {
                        return true;
                    }
                }
                false
            })
            .collect();

        if wanted.is_empty() {
            return Err("manifest filter produced no files".to_string());
        }

        let total: u64 = wanted.iter().map(|f| f.size).sum();
        let mut done: u64 = 0;
        let model_subdir = target_dir.join(&def.id);

        for entry in &wanted {
            // 仓库内路径形如 kokoro-int8-multi-lang-v1_1/xxx → 去掉顶层目录直接落到模型目录。
            let rel = match entry.path.split_once('/') {
                Some((_, rest)) => rest,
                None => entry.path.as_str(),
            };
            let dest = model_subdir.join(rel);
            if dest.exists() && std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) == entry.size
            {
                done += entry.size;
                continue;
            }
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            let url = format!("{}/{}", base, entry.path);
            self.download_to_file_with_offset(
                app,
                def,
                &url,
                &dest,
                is_modelscope,
                done,
                total,
            )
            .await?;
            done += entry.size;
        }
        Ok(())
    }

    /// 通用文件下载：断点续传 + 进度上报。
    async fn download_to_file(
        &self,
        app: &tauri::AppHandle,
        def: &VoiceModelDef,
        url: &str,
        target: impl AsRef<Path>,
        is_modelscope: bool,
    ) -> Result<PathBuf, String> {
        let target_path = target.as_ref().to_path_buf();
        let total = def.total_size;
        self.download_to_file_with_offset(app, def, url, &target_path, is_modelscope, 0, total)
            .await?;
        Ok(target_path)
    }

    async fn download_to_file_with_offset(
        &self,
        app: &tauri::AppHandle,
        def: &VoiceModelDef,
        url: &str,
        target: impl AsRef<Path>,
        is_modelscope: bool,
        base_done: u64,
        base_total: u64,
    ) -> Result<(), String> {
        let target = target.as_ref();
        let partial = target.with_extension("part");
        let existing: u64 = tokio::fs::metadata(&partial)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        let http = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 cn-codex")
            // 跟随 ModelScope LFS CDN 重定向。
            .redirect(reqwest::redirect::Policy::limited(8))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let mut request = http.get(url);
        if existing > 0 {
            request = request.header("Range", format!("bytes={existing}-"));
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("request {url}: {e}"))?;

        // 服务器不支持续传（返回 200 而非 206）时重头下载。
        let resume_from = if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            existing
        } else {
            0
        };
        if !response.status().is_success() {
            return Err(format!("download {url} status {}", response.status()));
        }

        let content_length = response.content_length().unwrap_or(0) + resume_from;
        let total = if base_total > 0 && def.kind == "manifest" {
            base_total
        } else if content_length > 0 {
            content_length
        } else {
            def.total_size
        };

        let mut file = if resume_from > 0 {
            let mut f = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&partial)
                .await
                .map_err(|e| format!("open partial: {e}"))?;
            f.seek(std::io::SeekFrom::End(0))
                .await
                .map_err(|e| format!("seek partial: {e}"))?;
            f
        } else {
            tokio::fs::File::create(&partial)
                .await
                .map_err(|e| format!("create partial: {e}"))?
        };

        let mut downloaded = resume_from;
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        let mut last_emit = Instant::now();
        let mut last_bytes = downloaded;
        let mut speed: i64 = -1;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("read stream: {e}"))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("write chunk: {e}"))?;
            downloaded += chunk.len() as u64;

            if last_emit.elapsed().as_millis() as u64 >= PROGRESS_INTERVAL_MS {
                let dt = last_emit.elapsed().as_secs_f64();
                if dt > 0.05 {
                    speed = ((downloaded - last_bytes) as f64 / dt) as i64;
                }
                last_emit = Instant::now();
                last_bytes = downloaded;
                let overall_done = base_done + downloaded;
                self.emit(app, def, "downloading", overall_done, total.max(overall_done), speed, None)
                    .await
                    .ok();
            }
        }
        file.flush().await.map_err(|e| format!("flush: {e}"))?;
        drop(file);

        // 大小校验（ModelScope LFS 场景 content_length 可信）。
        let final_size = tokio::fs::metadata(&partial)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if content_length > 0 && final_size != content_length {
            return Err(format!(
                "size mismatch: got {final_size}, expect {content_length}"
            ));
        }
        tokio::fs::rename(&partial, target)
            .await
            .map_err(|e| format!("rename: {e}"))?;

        let _ = is_modelscope;
        let overall_done = base_done + final_size;
        self.emit(app, def, "downloading", overall_done, total.max(overall_done), speed, None)
            .await
            .ok();
        Ok(())
    }
}

/// 判断清单中某路径是否为目录。
fn def_is_dir(files: &[ModelScopeFileEntry], path: &str) -> bool {
    let prefix = format!("{path}/");
    files.iter().any(|f| f.path.starts_with(&prefix))
}

/// 解压 tar.bz2 到目标目录（同步阻塞，调用方放 blocking 线程）。
pub fn extract_tar_bz2(archive: &Path, target_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {e}"))?;
    let decoder = bzip2::read::BzDecoder::new(std::io::BufReader::new(file));
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(false);
    tar.unpack(target_dir)
        .map_err(|e| format!("unpack: {e}"))
}

/// 归档内目录名归一化：Qwen3 包内顶层目录可能是
/// `sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25/`，解包后把它拍平到模型目录。
pub fn flatten_archive_dir(target_dir: &Path, ready_marker: &str) {
    if target_dir.join(ready_marker).exists() {
        return;
    }
    let entries = match std::fs::read_dir(target_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    if dirs.len() == 1 {
        let inner = &dirs[0];
        if inner.join(ready_marker).exists() || inner.join("tokens").exists() {
            move_dir_contents(inner, target_dir);
        }
    }
}

fn move_dir_contents(from: &Path, to: &Path) {
    let entries = match std::fs::read_dir(from) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let dest = to.join(entry.file_name());
        if std::fs::rename(entry.path(), &dest).is_err() {
            // 跨目录 rename 失败时尝试 copy + delete。
            if entry.path().is_dir() {
                let _ = std::fs::create_dir_all(&dest);
                move_dir_contents(&entry.path(), &dest);
                let _ = std::fs::remove_dir_all(entry.path());
            } else if std::fs::copy(entry.path(), &dest).is_ok() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let _ = std::fs::remove_dir(from);
}

const _: () = {
    // 保证 CHUNK 在未来扩展时使用。
};

#[allow(dead_code)]
fn unused_chunk_hint(_: usize) -> usize {
    CHUNK
}
