param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$answerPath = Join-Path $Workspace "answer.json"
if (-not (Test-Path $answerPath)) { return @{ pass = 0; reason = "missing answer.json" } }
try { $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$pass = if (([string]$ans.test_script -eq "test") -and ([string]$ans.runner -eq "vitest") -and ([bool]($ans.has_watch_script -eq $true)) -and ([string]$ans.rust_manifest -eq "src-tauri/Cargo.toml")) { 1 } else { 0 }
return @{ pass = $pass; reason = $(if ($pass) { "correct" } else { "field mismatch" }) }