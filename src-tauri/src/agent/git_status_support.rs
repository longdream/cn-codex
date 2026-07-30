use super::*;

pub(crate) fn merge_git_changes(
    changes: &mut Vec<FileChange>,
    before: &GitStatusSnapshot,
    after: &GitStatusSnapshot,
) {
    for (path, entry) in after {
        if before.get(path) == Some(entry) {
            continue;
        }

        push_file_change(
            changes,
            FileChange {
                path: path.clone(),
                action: git_status_to_action(&entry.status),
            },
        );
    }

    for (path, entry) in before {
        if after.contains_key(path) {
            continue;
        }

        push_file_change(
            changes,
            FileChange {
                path: path.clone(),
                action: git_status_disappeared_to_action(&entry.status),
            },
        );
    }
}


pub(crate) fn git_status_to_action(status: &str) -> String {
    if status.contains('D') {
        "deleted".to_string()
    } else if status.contains('A') || status == "??" {
        "created".to_string()
    } else if status.contains('R') {
        "renamed".to_string()
    } else {
        "modified".to_string()
    }
}


pub(crate) fn git_status_disappeared_to_action(status: &str) -> String {
    if status == "??" || status.contains('A') {
        "deleted".to_string()
    } else if status.contains('D') {
        "created".to_string()
    } else {
        "modified".to_string()
    }
}


pub(crate) async fn git_status_snapshot(cwd: &Path) -> GitStatusSnapshot {
    let mut cmd = Command::new("git");
    cmd.args(["status", "--porcelain"]).current_dir(cwd);
    #[cfg(windows)]
    cmd.no_console();
    let Ok(output) = cmd.output().await else {
        return GitStatusSnapshot::new();
    };

    if !output.status.success() {
        return GitStatusSnapshot::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(parse_git_status_line)
        .map(|(path, status)| {
            let fingerprint = file_fingerprint(cwd, &path);
            (
                path,
                GitStatusEntry {
                    status,
                    fingerprint,
                },
            )
        })
        .collect()
}


pub(crate) fn file_fingerprint(cwd: &Path, path: &str) -> Option<u64> {
    let path = cwd.join(path);
    if !path.is_file() {
        return None;
    }

    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}


pub(crate) fn parse_git_status_line(line: &str) -> Option<(String, String)> {
    if line.len() < 4 {
        return None;
    }

    let status = line.get(0..2)?.trim().to_string();
    let raw_path = line.get(3..)?.trim();
    if raw_path.is_empty() {
        return None;
    }

    let path = raw_path.rsplit(" -> ").next().unwrap_or(raw_path).trim();
    let path = decode_git_quoted_path(path).replace('\\', "/");

    Some((path, status))
}


pub(crate) fn decode_git_quoted_path(path: &str) -> String {
    let Some(inner) = path
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return path.to_string();
    };

    let input = inner.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'\\' || index + 1 >= input.len() {
            output.push(input[index]);
            index += 1;
            continue;
        }

        index += 1;
        let escaped = input[index];
        if (b'0'..=b'7').contains(&escaped) {
            let mut value = 0_u16;
            let mut digits = 0;
            while index < input.len() && digits < 3 && (b'0'..=b'7').contains(&input[index]) {
                value = value * 8 + u16::from(input[index] - b'0');
                index += 1;
                digits += 1;
            }
            if value <= u16::from(u8::MAX) {
                output.push(value as u8);
            } else {
                output.extend_from_slice(&input[index - digits..index]);
            }
            continue;
        }

        output.push(match escaped {
            b'a' => 0x07,
            b'b' => 0x08,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 0x0b,
            b'f' => 0x0c,
            b'r' => b'\r',
            b'\\' => b'\\',
            b'"' => b'"',
            other => other,
        });
        index += 1;
    }

    String::from_utf8(output).unwrap_or_else(|_| path.trim_matches('"').to_string())
}


