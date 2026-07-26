param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$answerPath = Join-Path $Workspace "answer.json"
if (-not (Test-Path $answerPath)) { return @{ pass = 0; reason = "missing answer.json" } }
try { $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$pass = if (([string]$ans.entrypoint -eq "src/main.tsx") -and ([string]$ans.ui_framework -eq "react") -and ([string]$ans.desktop_framework -eq "tauri") -and ([string]$ans.test_script -eq "test")) { 1 } else { 0 }
return @{ pass = $pass; reason = $(if ($pass) { "correct" } else { "field mismatch" }) }