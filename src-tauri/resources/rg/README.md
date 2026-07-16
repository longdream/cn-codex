# Bundled ripgrep (`rg`)

CN-Codex compiles the platform-specific `rg` binary in this directory into
the main application executable. It is an internal implementation detail of
the `code_search` tool and never depends on a system `rg` installation or PATH.

## Expected files

| Platform | File |
|----------|------|
| Windows x64 | `rg.exe` |
| Linux x64 (optional) | `rg` |
| macOS (optional) | `rg` |

## Runtime extraction

At compile time, Rust `include_bytes!` embeds the binary in CN-Codex. On the
first `code_search` call, CN-Codex verifies and extracts it into a private,
content-addressed cache under `%LOCALAPPDATA%/CN-Codex/bin/`. Subsequent calls
reuse the verified cached file.

Do not add this file to Tauri's external `bundle.resources`: release output
must contain the main CN-Codex executable without a sidecar `rg.exe`.

## Maintainer download

From the repo root:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-ripgrep.ps1
```

Approximate size impact: **~4–6 MB** for Windows `rg.exe`.
