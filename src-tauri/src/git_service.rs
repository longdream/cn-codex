use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::error::{AppError, AppResult};

#[cfg(windows)]
trait CommandNoConsole {
    fn no_console(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl CommandNoConsole for Command {
    fn no_console(&mut self) -> &mut Self {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        self.creation_flags(CREATE_NO_WINDOW)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct GitService {
    cwd: PathBuf,
}

impl GitService {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn resolve_cwd(default_cwd: &Path, override_cwd: Option<&str>) -> AppResult<PathBuf> {
        // Git 面板允许按前端传入目录执行；为空时回落到应用当前工作目录。
        let candidate = override_cwd
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| default_cwd.to_path_buf());
        let normalized = candidate.canonicalize().unwrap_or(candidate.clone());

        if !normalized.is_dir() {
            return Err(AppError::Custom(format!(
                "Git working directory is invalid: {}",
                normalized.to_string_lossy()
            )));
        }
        Ok(normalized)
    }

    pub async fn ensure_repository(&self) -> AppResult<()> {
        let output = self
            .run_allow_failure(&["rev-parse", "--is-inside-work-tree"], 8 * 1024)
            .await?;
        if output.exit_code != 0 || output.stdout.trim() != "true" {
            return Err(AppError::Custom(format!(
                "Directory is not a git repository: {}",
                self.cwd.to_string_lossy()
            )));
        }
        Ok(())
    }

    pub async fn run(&self, args: &[&str], max_output_bytes: usize) -> AppResult<GitCommandOutput> {
        let output = self.run_allow_failure(args, max_output_bytes).await?;
        if output.exit_code == 0 {
            return Ok(output);
        }

        let detail = if output.stderr.trim().is_empty() {
            format!(
                "git {} failed with exit code {}",
                args.join(" "),
                output.exit_code
            )
        } else {
            output.stderr.trim().to_string()
        };
        Err(AppError::Custom(detail))
    }

    pub async fn run_allow_failure(
        &self,
        args: &[&str],
        max_output_bytes: usize,
    ) -> AppResult<GitCommandOutput> {
        let mut command = Command::new("git");
        command
            .args(args)
            .current_dir(&self.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.no_console();

        let mut child = command
            .spawn()
            .map_err(|err| AppError::Custom(format!("Failed to spawn git: {err}")))?;

        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();
        let stdout_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut out) = child_stdout {
                let _ = out.read_to_end(&mut buf).await;
            }
            buf
        });
        let stderr_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut err) = child_stderr {
                let _ = err.read_to_end(&mut buf).await;
            }
            buf
        });

        match tokio::time::timeout(Duration::from_secs(30), child.wait()).await {
            Ok(Ok(status)) => {
                let stdout_bytes = stdout_handle.await.unwrap_or_default();
                let stderr_bytes = stderr_handle.await.unwrap_or_default();
                Ok(GitCommandOutput {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: truncate_bytes_to_string(&stdout_bytes, max_output_bytes),
                    stderr: truncate_bytes_to_string(&stderr_bytes, max_output_bytes / 2),
                })
            }
            Ok(Err(err)) => Err(AppError::Custom(format!(
                "Failed waiting git process: {err}"
            ))),
            Err(_) => {
                let _ = child.kill().await;
                stdout_handle.abort();
                stderr_handle.abort();
                Err(AppError::Custom(
                    "Git command timed out after 30 seconds".to_string(),
                ))
            }
        }
    }
}

fn truncate_bytes_to_string(bytes: &[u8], max_bytes: usize) -> String {
    let cap = max_bytes.max(1024);
    if bytes.len() <= cap {
        return String::from_utf8_lossy(bytes).to_string();
    }

    let mut text = String::from_utf8_lossy(&bytes[..cap]).to_string();
    text.push_str("\n...[truncated]...");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_cwd_rejects_non_directory_path() {
        let test_root =
            std::env::temp_dir().join(format!("cn-codex-git-service-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_root).unwrap();
        let file_path = test_root.join("not-dir.txt");
        std::fs::write(&file_path, "x").unwrap();

        let result =
            GitService::resolve_cwd(&test_root, Some(file_path.to_string_lossy().as_ref()));
        assert!(result.is_err());

        let _ = std::fs::remove_file(file_path);
        let _ = std::fs::remove_dir_all(test_root);
    }

    #[test]
    fn resolve_cwd_uses_override_directory_when_present() {
        let default_root =
            std::env::temp_dir().join(format!("cn-codex-default-{}", uuid::Uuid::new_v4()));
        let override_root =
            std::env::temp_dir().join(format!("cn-codex-override-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&default_root).unwrap();
        std::fs::create_dir_all(&override_root).unwrap();

        let resolved = GitService::resolve_cwd(
            &default_root,
            Some(override_root.to_string_lossy().as_ref()),
        )
        .unwrap();
        assert_eq!(resolved, override_root.canonicalize().unwrap());

        let _ = std::fs::remove_dir_all(default_root);
        let _ = std::fs::remove_dir_all(override_root);
    }

    #[test]
    fn truncate_bytes_marks_output_when_exceeding_limit() {
        let output = truncate_bytes_to_string("a".repeat(2048).as_bytes(), 10);
        assert!(output.contains("[truncated]"));
    }
}
