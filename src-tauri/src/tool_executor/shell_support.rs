use super::*;

/// 以短轮询方式等待子进程结束。
///
/// 说明：
/// - 不直接 `child.wait().await`，避免在等待期间长时间独占 child 的可变借用；
/// - 这样 interrupt 逻辑仍可获取 child 并执行 kill。
pub(crate) async fn wait_for_child_with_timeout(
    child: &Arc<Mutex<Child>>,
    timeout_ms: u64,
) -> WaitChildResult {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let wait_result = {
            let mut guard = child.lock().await;
            guard.try_wait()
        };

        match wait_result {
            Ok(Some(status)) => return WaitChildResult::Exited(status),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    return WaitChildResult::TimedOut;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return WaitChildResult::Failed(error.to_string()),
        }
    }
}


pub(crate) async fn collect_shell_stream_bytes<R>(mut reader: R, output: Arc<Mutex<Vec<u8>>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buf = [0u8; 4096];
    loop {
        let Ok(n) = reader.read(&mut buf).await else {
            break;
        };
        if n == 0 {
            break;
        }
        output.lock().await.extend_from_slice(&buf[..n]);
    }
}


pub(crate) async fn wait_for_shell_stream_task(handle: &mut tokio::task::JoinHandle<()>, timeout_ms: u64) {
    if tokio::time::timeout(Duration::from_millis(timeout_ms), &mut *handle)
        .await
        .is_err()
    {
        handle.abort();
    }
}


pub(crate) async fn terminate_shell_child(child: &Arc<Mutex<Child>>) {
    let pid = {
        let guard = child.lock().await;
        guard.id()
    };
    if let Some(pid) = pid {
        if let Err(error) = kill_process_tree(pid).await {
            info!("Failed to kill process tree {pid}, fallback to child.kill(): {error}");
        }
    }

    let mut guard = child.lock().await;
    let _ = guard.kill().await;
}


#[allow(dead_code)]
pub(crate) async fn kill_process_tree(pid: u32) -> Result<(), String> {
    let output = if cfg!(windows) {
        let pid = pid.to_string();
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", pid.as_str(), "/F", "/T"]);
        #[cfg(windows)]
        cmd.no_console();
        cmd.output().await
    } else {
        let pid = pid.to_string();
        Command::new("kill")
            .args(["-TERM", pid.as_str()])
            .output()
            .await
    }
    .map_err(|e| format!("Failed to stop process {pid}: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = decode_command_output_bytes(&output.stdout)
        .trim()
        .to_string();
    let stderr = decode_command_output_bytes(&output.stderr)
        .trim()
        .to_string();
    let details = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit code {}", output.status.code().unwrap_or(-1))
    };
    Err(format!("Failed to stop process {pid}: {details}"))
}


pub(crate) fn combine_stdout_stderr(stdout: &str, stderr: &str) -> String {
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.trim().to_string(),
        (true, false) => stderr.trim().to_string(),
        (false, false) => format!("{}\n[stderr]\n{}", stdout.trim(), stderr.trim()),
    }
}


pub(crate) fn resolve_command_cwd(base: &Path, cwd: Option<&str>) -> PathBuf {
    let Some(cwd) = cwd.filter(|value| !value.trim().is_empty()) else {
        let base_display = normalize_windows_verbatim_prefix(&base.to_string_lossy());
        return PathBuf::from(windows_display_path_to_access_path(&base_display));
    };

    let normalized = normalize_windows_verbatim_prefix(cwd);
    let path = PathBuf::from(&normalized);
    let resolved = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    let display = normalize_windows_verbatim_prefix(&resolved.to_string_lossy());
    PathBuf::from(windows_display_path_to_access_path(&display))
}


pub(crate) fn shell_command_display(command: &ShellCommandArg) -> String {
    match command {
        ShellCommandArg::Script(script) => script.trim().to_string(),
        ShellCommandArg::Argv(argv) => argv.join(" ").trim().to_string(),
    }
}


pub(crate) fn powershell_command_validation_error(command: &str) -> Option<String> {
    for (index, line) in command.lines().enumerate() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        let checkbox_placeholder = ["[ ]", "[x]"].iter().any(|prefix| {
            lower.strip_prefix(prefix).is_some_and(|rest| {
                let rest = rest.trim_start();
                rest.is_empty() || rest.starts_with('|') || rest.starts_with('#')
            })
        });
        let broken_closing_placeholder = trimmed.strip_prefix(']').is_some_and(|rest| {
            let rest = rest.trim_start();
            rest.is_empty() || rest.starts_with('|') || rest.starts_with('#')
        });

        if checkbox_placeholder || broken_closing_placeholder || trimmed.starts_with('|') {
            return Some(format!(
                "Error: invalid PowerShell command on line {}. The command argument must be one non-empty string containing a complete executable script, and every pipeline must begin with an input-producing command.",
                index + 1
            ));
        }

        if trimmed.contains("$*.") {
            return Some(format!(
                "Error: invalid PowerShell current-object reference on line {}. Inside Where-Object or ForEach-Object, use '$_' as the current pipeline object.",
                index + 1
            ));
        }
    }

    None
}


pub(crate) fn shell_file_editing_violation(command: &str) -> Option<String> {
    let lower = command.to_ascii_lowercase();
    let blocked_patterns = [
        "set-content",
        "out-file",
        "add-content",
        "writealltext(",
        "writeallbytes(",
        ".write_text(",
        ".write_bytes(",
        "sed -i",
    ];
    let matched = blocked_patterns
        .iter()
        .find(|pattern| lower.contains(**pattern))?;
    Some(format!(
        "Error: shell-based file editing is disabled ({matched}). Use apply_patch for existing files or write_file for new UTF-8 files. This prevents Chinese and other non-ASCII text from being corrupted by shell encoding defaults."
    ))
}


pub(crate) fn resolve_shell_timeout_ms(args: &ShellArgs) -> Result<u64, String> {
    let timeout_ms = args
        .timeout_ms
        .or(args.block_until_ms)
        .unwrap_or(SHELL_TIMEOUT_DEFAULT_MS);
    if timeout_ms < SHELL_TIMEOUT_MIN_MS {
        return Err(format!(
            "Error: shell timeout must be at least {SHELL_TIMEOUT_MIN_MS} ms (received {timeout_ms})."
        ));
    }
    if timeout_ms > SHELL_TIMEOUT_MAX_MS {
        return Err(format!(
            "Error: shell timeout exceeds {SHELL_TIMEOUT_MAX_MS} ms (received {timeout_ms})."
        ));
    }
    Ok(timeout_ms)
}


pub(crate) fn inject_powershell_utf8_prefix(script: &str) -> String {
    if script
        .trim_start()
        .starts_with(POWERSHELL_UTF8_PREFIX_MARKER)
    {
        script.to_string()
    } else {
        format!("{POWERSHELL_UTF8_OUTPUT_PREFIX}{script}")
    }
}


pub(crate) fn shell_requires_permission_approval(args: &ShellArgs) -> bool {
    normalize_sandbox_permissions(args.sandbox_permissions.as_deref())
        .is_some_and(|value| value != "use_default")
}


pub(crate) fn validate_shell_permission_args(args: &ShellArgs) -> Result<(), String> {
    let Some(permission) = normalize_sandbox_permissions(args.sandbox_permissions.as_deref())
    else {
        return Ok(());
    };

    match permission.as_str() {
        "use_default" => {
            if args.additional_permissions.is_some() {
                return Err(
                    "Error: additional_permissions requires sandbox_permissions: with_additional_permissions"
                        .to_string(),
                );
            }
        }
        "with_additional_permissions" => {
            let Some(profile) = &args.additional_permissions else {
                return Err(
                    "Error: with_additional_permissions requires additional_permissions"
                        .to_string(),
                );
            };
            if !profile.is_object() || profile.as_object().is_some_and(|object| object.is_empty()) {
                return Err("Error: additional_permissions must be a non-empty object".to_string());
            }
        }
        "require_escalated" => {}
        other => {
            return Err(format!(
                "Error: unsupported sandbox_permissions value '{other}'"
            ));
        }
    }

    Ok(())
}


pub(crate) fn normalize_sandbox_permissions(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        match value {
            "useDefault" => "use_default",
            "requireEscalated" => "require_escalated",
            "withAdditionalPermissions" => "with_additional_permissions",
            other => other,
        }
        .to_string(),
    )
}


pub(crate) fn shell_program_and_args_windows(script: &str, login: Option<bool>) -> (String, Vec<String>) {
    let ps_script = inject_powershell_utf8_prefix(&script.replace(" && ", "; "));
    let mut args = Vec::new();
    if login == Some(false) {
        args.push("-NoProfile".to_string());
    }
    args.push("-ExecutionPolicy".to_string());
    args.push("Bypass".to_string());
    args.push("-Command".to_string());
    args.push(ps_script);
    ("powershell.exe".to_string(), args)
}


pub(crate) fn shell_program_and_args_unix(script: &str, login: Option<bool>) -> (String, Vec<String>) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let shell_name = Path::new(&shell)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("sh");
    let supports_login = matches!(shell_name, "bash" | "zsh" | "fish");
    let flag = if login != Some(false) && supports_login {
        "-lc"
    } else {
        "-c"
    };
    (shell, vec![flag.to_string(), script.to_string()])
}


pub(crate) fn exec_command_program_and_args(args: &ExecCommandArgs) -> (String, Vec<String>) {
    if cfg!(target_os = "windows") {
        let shell = args
            .shell
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("powershell.exe")
            .to_string();
        let shell_name = Path::new(&shell)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if shell_name == "cmd.exe" || shell_name == "cmd" {
            return (shell, vec!["/C".to_string(), args.cmd.clone()]);
        }
        let command_script = if shell_name.contains("powershell")
            || shell_name == "pwsh.exe"
            || shell_name == "pwsh"
        {
            inject_powershell_utf8_prefix(&args.cmd)
        } else {
            args.cmd.clone()
        };
        let mut shell_args = Vec::new();
        if args.login == Some(false) {
            shell_args.push("-NoProfile".to_string());
        }
        shell_args.push("-ExecutionPolicy".to_string());
        shell_args.push("Bypass".to_string());
        shell_args.push("-Command".to_string());
        shell_args.push(command_script);
        (shell, shell_args)
    } else {
        let shell = args
            .shell
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()));
        let shell_name = Path::new(&shell)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("sh");
        let supports_login = matches!(shell_name, "bash" | "zsh" | "fish");
        let flag = if args.login != Some(false) && supports_login {
            "-lc"
        } else {
            "-c"
        };
        (shell, vec![flag.to_string(), args.cmd.clone()])
    }
}


pub(crate) fn exec_requires_permission_approval(args: &ExecCommandArgs) -> bool {
    normalize_sandbox_permissions(args.sandbox_permissions.as_deref())
        .is_some_and(|value| value != "use_default")
}


pub(crate) fn validate_exec_permission_args(args: &ExecCommandArgs) -> Result<(), String> {
    validate_permission_override_args(
        args.sandbox_permissions.as_deref(),
        args.additional_permissions.as_ref(),
    )
}


pub(crate) fn validate_permission_override_args(
    sandbox_permissions: Option<&str>,
    additional_permissions: Option<&serde_json::Value>,
) -> Result<(), String> {
    let Some(permission) = normalize_sandbox_permissions(sandbox_permissions) else {
        return Ok(());
    };

    match permission.as_str() {
        "use_default" => {
            if additional_permissions.is_some() {
                return Err(
                    "Error: additional_permissions requires sandbox_permissions: with_additional_permissions"
                        .to_string(),
                );
            }
        }
        "with_additional_permissions" => {
            let Some(profile) = additional_permissions else {
                return Err(
                    "Error: with_additional_permissions requires additional_permissions"
                        .to_string(),
                );
            };
            if !profile.is_object() || profile.as_object().is_some_and(|object| object.is_empty()) {
                return Err("Error: additional_permissions must be a non-empty object".to_string());
            }
        }
        "require_escalated" => {}
        other => {
            return Err(format!(
                "Error: unsupported sandbox_permissions value '{other}'"
            ));
        }
    }

    Ok(())
}


pub(crate) async fn collect_exec_output<R>(
    mut reader: R,
    output: Arc<Mutex<String>>,
    prefix: Option<&'static str>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buf = [0u8; 4096];
    let mut wrote_prefix = false;
    let mut decoder = crate::utf8_stream::Utf8StreamDecoder::new();
    loop {
        let Ok(n) = reader.read(&mut buf).await else {
            break;
        };
        if n == 0 {
            break;
        }
        let mut chunk = String::new();
        decoder.push(&mut chunk, &buf[..n]);
        if chunk.is_empty() {
            continue;
        }
        let mut output = output.lock().await;
        if let Some(prefix) = prefix
            && !wrote_prefix
        {
            output.push_str(prefix);
            wrote_prefix = true;
        }
        output.push_str(&chunk);
    }

    let mut tail = String::new();
    decoder.finish(&mut tail);
    if !tail.is_empty() {
        let mut output = output.lock().await;
        if let Some(prefix) = prefix
            && !wrote_prefix
        {
            output.push_str(prefix);
        }
        output.push_str(&tail);
    }
}


pub(crate) async fn exec_session_snapshot(
    record: &ExecSessionRecord,
    max_output_tokens: Option<usize>,
) -> serde_json::Value {
    let exit_code = *record.exit_code.lock().await;
    let output = record.output.lock().await;
    let mut cursor = record.cursor.lock().await;
    let start = (*cursor).min(output.len());
    let new_output = output[start..].to_string();
    *cursor = output.len();
    drop(cursor);
    drop(output);

    let output_limit = max_output_tokens_to_chars(max_output_tokens);
    let truncated = truncate_output(&new_output, output_limit);
    let original_token_count = estimate_token_count(&new_output);
    let wall_time_seconds = (now_millis().saturating_sub(record.started_at_ms) as f64) / 1000.0;

    let mut value = serde_json::json!({
        "wall_time_seconds": wall_time_seconds,
        "original_token_count": original_token_count,
        "output": truncated,
        "command": record.command.clone(),
        "cwd": record.cwd.clone(),
    });

    if let Some(exit_code) = exit_code {
        value["exit_code"] = serde_json::json!(exit_code);
    } else {
        value["session_id"] = serde_json::json!(record.id);
    }
    value
}


pub(crate) async fn close_exec_session_record(record: ExecSessionRecord) -> serde_json::Value {
    let previous_exit_code = *record.exit_code.lock().await;
    let was_running = previous_exit_code.is_none();
    let mut close_error = None;

    {
        let mut stdin = record.stdin.lock().await;
        stdin.take();
    }

    if was_running {
        match record.process_id {
            Some(pid) => {
                if let Err(error) = kill_process_tree(pid).await {
                    close_error = Some(error);
                }
            }
            None => {
                close_error = Some("Exec session process id is unavailable".to_string());
            }
        }

        let mut exit_code = record.exit_code.lock().await;
        if exit_code.is_none() {
            *exit_code = Some(if close_error.is_none() { 130 } else { -1 });
        }
    }

    {
        let mut output = record.output.lock().await;
        let message = if was_running {
            if let Some(error) = close_error.as_deref() {
                format!("\n[session close]\nFailed to stop exec session: {error}\n")
            } else {
                "\n[session close]\nExec session closed and process tree stop was requested.\n"
                    .to_string()
            }
        } else {
            "\n[session close]\nExec session was already finished and has been removed.\n"
                .to_string()
        };
        output.push_str(&message);
    }

    let mut value = exec_session_snapshot(&record, None).await;
    value["session_id"] = serde_json::json!(record.id);
    value["closed"] = serde_json::json!(close_error.is_none());
    value["was_running"] = serde_json::json!(was_running);
    value["process_id"] = serde_json::json!(record.process_id);
    if let Some(previous_exit_code) = previous_exit_code {
        value["previous_exit_code"] = serde_json::json!(previous_exit_code);
    }
    if let Some(close_error) = close_error {
        value["error"] = serde_json::json!(close_error);
    }
    value
}


pub(crate) fn exec_yield_duration(value: Option<u64>, after_write: bool) -> Duration {
    let default = if after_write { 250 } else { 10_000 };
    Duration::from_millis(value.unwrap_or(default).clamp(250, 300_000))
}


pub(crate) fn max_output_tokens_to_chars(value: Option<usize>) -> usize {
    value.unwrap_or(10_000).clamp(100, 50_000).saturating_mul(4)
}


pub(crate) fn estimate_token_count(value: &str) -> usize {
    value.chars().count().div_ceil(4)
}


pub(crate) fn decode_command_output_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_owned();
    }

    let encoding = detect_output_encoding(bytes);
    let (decoded, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        decoded.into_owned()
    }
}


pub(crate) fn detect_output_encoding(bytes: &[u8]) -> &'static Encoding {
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let encoding = detector.guess(None, Utf8Detection::Allow);
    if encoding == IBM866 && looks_like_windows_1252_punctuation(bytes) {
        return WINDOWS_1252;
    }
    encoding
}


pub(crate) fn looks_like_windows_1252_punctuation(bytes: &[u8]) -> bool {
    let mut saw_extended_punctuation = false;
    let mut saw_ascii_word = false;

    for &byte in bytes {
        if byte >= 0xA0 {
            return false;
        }
        if (0x80..=0x9F).contains(&byte) {
            if !is_windows_1252_punct(byte) {
                return false;
            }
            saw_extended_punctuation = true;
        }
        if byte.is_ascii_alphabetic() {
            saw_ascii_word = true;
        }
    }

    saw_extended_punctuation && saw_ascii_word
}


pub(crate) fn is_windows_1252_punct(byte: u8) -> bool {
    WINDOWS_1252_PUNCT_BYTES.contains(&byte)
}


pub(crate) fn normalize_windows_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r#"\\?\UNC\"#) {
        return format!(r#"\\{rest}"#);
    }
    if let Some(rest) = path.strip_prefix(r#"\\.\UNC\"#) {
        return format!(r#"\\{rest}"#);
    }
    if let Some(rest) = path.strip_prefix(r#"\\?\"#) {
        return rest.to_string();
    }
    if let Some(rest) = path.strip_prefix(r#"\\.\"#)
        && is_windows_drive_absolute_path(rest)
    {
        return rest.to_string();
    }
    path.to_string()
}


pub(crate) fn is_windows_drive_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}


pub(crate) fn windows_display_path_to_access_path(path: &str) -> String {
    if !cfg!(windows) {
        return path.to_string();
    }

    const WINDOWS_LONG_PATH_THRESHOLD: usize = 240;

    if path.starts_with(r#"\\?\"#) || path.starts_with(r#"\\.\"#) {
        return path.to_string();
    }
    if let Some(rest) = path.strip_prefix(r#"\\"#) {
        if path.len() >= WINDOWS_LONG_PATH_THRESHOLD {
            return format!(r#"\\?\UNC\{rest}"#);
        }
        return path.to_string();
    }
    if is_windows_drive_absolute_path(path) {
        if path.len() >= WINDOWS_LONG_PATH_THRESHOLD {
            return format!(r#"\\?\{path}"#);
        }
        return path.to_string();
    }
    path.to_string()
}


pub(crate) fn normalize_windows_paths_in_text(text: &str) -> String {
    text.lines()
        .map(|line| line.replace(r#"\\?\UNC\"#, r#"\\"#).replace(r#"\\?\"#, ""))
        .collect::<Vec<_>>()
        .join("\n")
}


pub(crate) fn format_shell_partial_output(stdout: &str, stderr: &str) -> String {
    let combined = combine_stdout_stderr(stdout, stderr);
    if combined.trim().is_empty() {
        return combined;
    }
    let normalized = normalize_windows_paths_in_text(&combined);
    append_shell_artifact_hints(&normalized)
}


pub(crate) fn format_shell_command_output(exit_code: i32, stdout: &str, stderr: &str) -> String {
    let partial = format_shell_partial_output(stdout, stderr);
    if exit_code == 0 {
        partial
    } else if partial.trim().is_empty() {
        format!("[exit code: {exit_code}]")
    } else {
        format!("[exit code: {exit_code}]\n{partial}")
    }
}


pub(crate) fn append_shell_artifact_hints(output: &str) -> String {
    if output.contains("[artifact hints]") {
        return output.to_string();
    }
    let hints = extract_shell_artifact_hints(output, 12);
    if hints.is_empty() {
        return output.to_string();
    }

    let mut enriched = output.to_string();
    enriched.push_str("\n\n[artifact hints]\n");
    for hint in hints {
        enriched.push_str("- ");
        enriched.push_str(&hint);
        enriched.push('\n');
    }
    enriched
}


pub(crate) fn extract_shell_artifact_hints(output: &str, max_lines: usize) -> Vec<String> {
    let mut hints = Vec::new();
    let mut seen = BTreeSet::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized_line = normalize_windows_paths_in_text(trimmed);
        if !looks_like_shell_priority_line(&normalized_line) {
            continue;
        }
        if seen.insert(normalized_line.clone()) {
            hints.push(normalized_line.clone());
            if hints.len() >= max_lines {
                break;
            }
        }

        // 这里额外回传 display_path -> access_path 映射，避免后续二次读取时
        // 因 `\\?\` 前缀或 UNC 长路径形态差异导致“展示可见但读取失败”。
        if let Some(path) = extract_first_path_candidate(&normalized_line) {
            let display = normalize_windows_verbatim_prefix(&path);
            let access = windows_display_path_to_access_path(&display);
            if access != display {
                let mapping = format!("path mapping: {display} -> {access}");
                if seen.insert(mapping.clone()) {
                    hints.push(mapping);
                    if hints.len() >= max_lines {
                        break;
                    }
                }
            }
        }
    }
    hints
}


pub(crate) fn looks_like_shell_priority_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let keywords = [
        "artifact",
        "result_extract.json",
        "prepared_payload.json",
        "source.md",
        "chunks",
        "chapter",
        "summary",
        "output_path",
        "result_path",
        "cache",
        "标书",
        "章节",
        "摘要",
        "路径",
    ];
    keywords.iter().any(|keyword| lower.contains(keyword))
        || line.contains(":\\")
        || line.contains(":/")
        || line.contains(r#"\\"#)
}


pub(crate) fn extract_first_path_candidate(line: &str) -> Option<String> {
    let separators = [
        '"', '\'', ',', ';', '(', ')', '[', ']', '{', '}', '<', '>', '\t', '\r', '\n',
    ];
    for raw in line.split(|ch: char| ch.is_whitespace() || separators.contains(&ch)) {
        let candidate = raw.trim_matches(|ch: char| matches!(ch, '.' | ':' | '!' | '?'));
        if candidate.is_empty() {
            continue;
        }
        let normalized = normalize_windows_verbatim_prefix(candidate);
        if normalized.contains(":\\")
            || normalized.contains(":/")
            || normalized.starts_with(r#"\\"#)
        {
            return Some(normalized);
        }
    }
    None
}


pub(crate) fn truncate_shell_output(output: &str, max_chars: usize) -> String {
    if output.len() <= max_chars {
        return output.to_string();
    }

    let priority_lines = extract_shell_artifact_hints(output, 12);
    if priority_lines.is_empty() {
        return truncate_output(output, max_chars);
    }

    let mut priority_block = String::from("[priority lines]\n");
    for line in priority_lines {
        priority_block.push_str("- ");
        priority_block.push_str(&line);
        priority_block.push('\n');
    }

    let reserve = priority_block.len().saturating_add(220);
    let core_budget = max_chars.saturating_sub(reserve).max(max_chars / 3);
    let head_budget = core_budget / 2;
    let tail_budget = core_budget.saturating_sub(head_budget);
    let head = take_prefix_chars(output, head_budget);
    let tail = take_suffix_chars(output, tail_budget);
    let truncated = output
        .chars()
        .count()
        .saturating_sub(head.chars().count().saturating_add(tail.chars().count()));

    let merged = format!(
        "{head}\n\n... [truncated {truncated} chars; kept priority lines] ...\n\n{priority_block}\n{tail}"
    );
    if merged.len() > max_chars.saturating_add(512) {
        truncate_output(&merged, max_chars)
    } else {
        merged
    }
}


pub(crate) fn take_prefix_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}


pub(crate) fn take_suffix_chars(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}


pub(crate) fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let half = max_chars / 2;
        let start: String = s.chars().take(half).collect();
        let end: String = s
            .chars()
            .rev()
            .take(half)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!(
            "{start}\n\n... [truncated {remaining} chars] ...\n\n{end}",
            remaining = s.len() - max_chars
        )
    }
}


impl ToolExecutor {
    pub(crate) async fn exec_shell(
        &self,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ShellArgs = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid {tool_name} args: {e}"))
        })?;

        let cmd_display = shell_command_display(&args.command);
        if cmd_display.trim().is_empty() {
            return Ok("Error: empty command".to_string());
        }
        if cfg!(target_os = "windows") {
            if let Some(msg) = powershell_command_validation_error(&cmd_display) {
                self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
                return Ok(msg);
            }
        }
        if let Some(msg) = shell_file_editing_violation(&cmd_display) {
            self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
            self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
            return Ok(msg);
        }

        let workdir = resolve_command_cwd(&self.cwd, args.workdir.as_deref());
        if !workdir.is_dir() {
            let msg = format!(
                "Error: workdir does not exist or is not a directory: {}",
                workdir.display()
            );
            self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
            self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
            return Ok(msg);
        }

        if let Err(msg) = validate_shell_permission_args(&args) {
            self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
            self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
            return Ok(msg);
        }

        let timeout_ms = match resolve_shell_timeout_ms(&args) {
            Ok(timeout_ms) => timeout_ms,
            Err(msg) => {
                self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
                return Ok(msg);
            }
        };

        info!("Executing shell: {cmd_display}");

        self.emit_tool_start(app_handle, thread_id, call_id, tool_name, &cmd_display);

        if shell_requires_permission_approval(&args)
            && !self
                .additional_permissions_preapproved(
                    args.sandbox_permissions.as_deref(),
                    args.additional_permissions.as_ref(),
                )
                .await
        {
            let request_id = RequestId::String(format!("approval-{call_id}"));
            let reason = args
                .justification
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Approve command permission override");
            let payload = serde_json::json!({
                "requestId": request_id,
                "method": "commandExecution",
                "params": {
                    "command": cmd_display,
                    "cwd": normalize_windows_verbatim_prefix(&workdir.to_string_lossy()),
                    "reason": reason,
                    "sandbox_permissions": args.sandbox_permissions.clone(),
                    "prefix_rule": args.prefix_rule.clone(),
                    "additional_permissions": args.additional_permissions.clone(),
                }
            });
            let _ = app_handle.emit("server-request", payload);
            if let Err(msg) = wait_for_approval_result(app_handle, &request_id, 600_000).await {
                let output = format!("Command rejected: {msg}");
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &output);
                return Ok(output);
            }
        }

        let (program, cmd_args) = if cfg!(target_os = "windows") {
            shell_program_and_args_windows(&cmd_display, args.login)
        } else {
            shell_program_and_args_unix(&cmd_display, args.login)
        };

        let mut cmd = Command::new(&program);
        cmd.args(&cmd_args)
            .current_dir(&workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.no_console();
        let mut child = cmd
            .spawn()
            .map_err(|e| crate::error::AppError::Custom(format!("Failed to spawn command: {e}")))?;

        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::new(Mutex::new(Vec::new()));
        let stdout_collector = stdout_buffer.clone();
        let stderr_collector = stderr_buffer.clone();

        let mut stdout_handle = tokio::spawn(async move {
            if let Some(out) = child_stdout {
                collect_shell_stream_bytes(out, stdout_collector).await;
            }
        });
        let mut stderr_handle = tokio::spawn(async move {
            if let Some(err) = child_stderr {
                collect_shell_stream_bytes(err, stderr_collector).await;
            }
        });

        let child = Arc::new(Mutex::new(child));
        self.register_active_tool_process(thread_id, call_id, tool_name, child.clone())
            .await;

        let result = match wait_for_child_with_timeout(&child, timeout_ms).await {
            WaitChildResult::Exited(status) => {
                wait_for_shell_stream_task(&mut stdout_handle, 1_500).await;
                wait_for_shell_stream_task(&mut stderr_handle, 1_500).await;
                let stdout = decode_command_output_bytes(&stdout_buffer.lock().await);
                let stderr = decode_command_output_bytes(&stderr_buffer.lock().await);
                let exit_code = status.code().unwrap_or(-1);
                let output = format_shell_command_output(exit_code, &stdout, &stderr);
                let truncated = truncate_shell_output(&output, TOOL_OUTPUT_SHELL_MAX_CHARS);
                self.emit_tool_end(
                    app_handle, thread_id, call_id, tool_name, exit_code, &truncated,
                );
                Ok(truncated)
            }
            WaitChildResult::Failed(error) => {
                wait_for_shell_stream_task(&mut stdout_handle, 300).await;
                wait_for_shell_stream_task(&mut stderr_handle, 300).await;
                let stdout = decode_command_output_bytes(&stdout_buffer.lock().await);
                let stderr = decode_command_output_bytes(&stderr_buffer.lock().await);
                let partial = truncate_shell_output(
                    &format_shell_partial_output(&stdout, &stderr),
                    TOOL_OUTPUT_SHELL_PARTIAL_MAX_CHARS,
                );
                let msg = if partial.trim().is_empty() {
                    format!("Failed to wait for command: {error}")
                } else {
                    format!(
                        "Failed to wait for command: {error}\n\nPartial output captured before failure:\n{partial}"
                    )
                };
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, -1, &msg);
                Ok(msg)
            }
            WaitChildResult::TimedOut => {
                terminate_shell_child(&child).await;
                wait_for_shell_stream_task(&mut stdout_handle, 1_200).await;
                wait_for_shell_stream_task(&mut stderr_handle, 1_200).await;
                info!("Shell command timed out after {timeout_ms} ms: {cmd_display}");
                let stdout = decode_command_output_bytes(&stdout_buffer.lock().await);
                let stderr = decode_command_output_bytes(&stderr_buffer.lock().await);
                let partial = truncate_shell_output(
                    &format_shell_partial_output(&stdout, &stderr),
                    TOOL_OUTPUT_SHELL_PARTIAL_MAX_CHARS,
                );
                let msg = if partial.trim().is_empty() {
                    format!(
                        "Command timed out after {timeout_ms} ms.\nThe command '{cmd_display}' did not complete before the deadline and was terminated.\nNo partial output was captured."
                    )
                } else {
                    format!(
                        "Command timed out after {timeout_ms} ms.\nThe command '{cmd_display}' did not complete before the deadline and was terminated.\n\nPartial output captured before termination:\n{partial}"
                    )
                };
                self.emit_tool_end(app_handle, thread_id, call_id, tool_name, 124, &msg);
                Ok(msg)
            }
        };

        self.unregister_active_tool_process(thread_id, call_id)
            .await;
        result
    }


    pub(crate) async fn exec_command(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ExecCommandArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid exec_command args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
                return Ok(msg);
            }
        };

        let cmd = args.cmd.trim();
        if cmd.is_empty() {
            let msg = "Error: exec_command cmd must not be empty".to_string();
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", "empty");
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }
        let uses_powershell = cfg!(target_os = "windows")
            && args
                .shell
                .as_deref()
                .map(|shell| {
                    let name = Path::new(shell)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    name.contains("powershell") || name == "pwsh" || name == "pwsh.exe"
                })
                .unwrap_or(true);
        if uses_powershell {
            if let Some(msg) = powershell_command_validation_error(cmd) {
                self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
                return Ok(msg);
            }
        }
        if let Some(msg) = shell_file_editing_violation(cmd) {
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }

        let workdir = resolve_command_cwd(&self.cwd, args.workdir.as_deref());
        if !workdir.is_dir() {
            let msg = format!(
                "Error: workdir does not exist or is not a directory: {}",
                workdir.display()
            );
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }

        if let Err(msg) = validate_exec_permission_args(&args) {
            self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);
            self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
            return Ok(msg);
        }

        self.emit_tool_start(app_handle, thread_id, call_id, "exec_command", cmd);

        if exec_requires_permission_approval(&args)
            && !self
                .additional_permissions_preapproved(
                    args.sandbox_permissions.as_deref(),
                    args.additional_permissions.as_ref(),
                )
                .await
        {
            let request_id = RequestId::String(format!("approval-{call_id}"));
            let reason = args
                .justification
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Approve command permission override");
            let payload = serde_json::json!({
                "requestId": request_id,
                "method": "commandExecution",
                "params": {
                    "command": cmd,
                    "cwd": normalize_windows_verbatim_prefix(&workdir.to_string_lossy()),
                    "reason": reason,
                    "sandbox_permissions": args.sandbox_permissions.clone(),
                    "prefix_rule": args.prefix_rule.clone(),
                    "additional_permissions": args.additional_permissions.clone(),
                }
            });
            let _ = app_handle.emit("server-request", payload);
            if let Err(msg) = wait_for_approval_result(app_handle, &request_id, 600_000).await {
                let output = format!("Command rejected: {msg}");
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &output);
                return Ok(output);
            }
        }

        let (program, cmd_args) = exec_command_program_and_args(&args);
        let mut spawn_cmd = Command::new(&program);
        spawn_cmd
            .args(&cmd_args)
            .current_dir(&workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        spawn_cmd.no_console();
        let mut child = match spawn_cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let msg = format!("Failed to spawn command: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "exec_command", -1, &msg);
                return Ok(msg);
            }
        };

        let session_id = self.next_exec_session_id.fetch_add(1, Ordering::Relaxed);
        let process_id = child.id();
        let output = Arc::new(Mutex::new(String::new()));
        let cursor = Arc::new(Mutex::new(0usize));
        let exit_code = Arc::new(Mutex::new(None));
        let stdin = Arc::new(Mutex::new(child.stdin.take()));

        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(collect_exec_output(stdout, output.clone(), None));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(collect_exec_output(
                stderr,
                output.clone(),
                Some("[stderr]\n"),
            ));
        }

        let exit_code_for_task = exit_code.clone();
        tokio::spawn(async move {
            let code = match child.wait().await {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => -1,
            };
            let mut guard = exit_code_for_task.lock().await;
            *guard = Some(code);
        });

        let record = ExecSessionRecord {
            id: session_id,
            process_id,
            command: cmd.to_string(),
            cwd: normalize_windows_verbatim_prefix(&workdir.to_string_lossy()),
            started_at_ms: now_millis(),
            output,
            cursor,
            exit_code,
            stdin,
        };

        self.exec_sessions
            .lock()
            .await
            .insert(session_id, record.clone());

        let yield_time = exec_yield_duration(args.yield_time_ms.or(args.timeout_ms), false);
        tokio::time::sleep(yield_time).await;

        let result = exec_session_snapshot(&record, args.max_output_tokens).await;
        if result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .is_some()
        {
            self.exec_sessions.lock().await.remove(&session_id);
        }
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        let exit_code_for_event = result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value as i32)
            .unwrap_or(0);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "exec_command",
            exit_code_for_event,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_write_stdin(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: WriteStdinArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid write_stdin args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "write_stdin", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
                return Ok(msg);
            }
        };

        let display = format!("session {}", args.session_id);
        self.emit_tool_start(app_handle, thread_id, call_id, "write_stdin", &display);
        let Some(record) = self
            .exec_sessions
            .lock()
            .await
            .get(&args.session_id)
            .cloned()
        else {
            let msg = format!("Unknown exec session: {}", args.session_id);
            self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
            return Ok(msg);
        };

        let wrote_chars = args.chars.as_deref().is_some_and(|chars| !chars.is_empty());
        if let Some(chars) = args.chars.as_deref().filter(|chars| !chars.is_empty()) {
            let mut stdin = record.stdin.lock().await;
            let Some(stdin) = stdin.as_mut() else {
                let msg = format!("Exec session {} is not accepting stdin", args.session_id);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
                return Ok(msg);
            };
            if let Err(e) = stdin.write_all(chars.as_bytes()).await {
                let msg = format!("Failed to write to exec session {}: {e}", args.session_id);
                self.emit_tool_end(app_handle, thread_id, call_id, "write_stdin", -1, &msg);
                return Ok(msg);
            }
        }

        tokio::time::sleep(exec_yield_duration(args.yield_time_ms, wrote_chars)).await;

        let result = exec_session_snapshot(&record, args.max_output_tokens).await;
        if result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .is_some()
        {
            self.exec_sessions.lock().await.remove(&args.session_id);
        }
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        let exit_code_for_event = result
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value as i32)
            .unwrap_or(0);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "write_stdin",
            exit_code_for_event,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_close_exec_session(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: CloseExecSessionArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid close_exec_session args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "close_exec_session",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "close_exec_session",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        let display = format!("session {}", args.session_id);
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "close_exec_session",
            &display,
        );
        let Some(record) = self.exec_sessions.lock().await.remove(&args.session_id) else {
            let msg = format!("Unknown exec session: {}", args.session_id);
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "close_exec_session",
                -1,
                &msg,
            );
            return Ok(msg);
        };

        let result = close_exec_session_record(record).await;
        let exit_code_for_event = if result
            .get("closed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            0
        } else {
            -1
        };
        let output = serde_json::to_string_pretty(&result).unwrap_or_default();
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "close_exec_session",
            exit_code_for_event,
            &output,
        );
        Ok(output)
    }

}
