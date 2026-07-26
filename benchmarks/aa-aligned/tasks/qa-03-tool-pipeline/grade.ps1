param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$answerPath = Join-Path $Workspace "answer.json"
if (-not (Test-Path $answerPath)) { return @{ pass = 0; reason = "missing answer.json" } }
try { $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$pass = if (([bool]($ans.has_apply_patch -eq $true)) -and ([bool]($ans.has_browser_run -eq $true)) -and ([bool]($ans.has_tool_search -eq $true)) -and ([string]$ans.source_file -eq "src/hooks/useTauriEvents.ts")) { 1 } else { 0 }
return @{ pass = $pass; reason = $(if ($pass) { "correct" } else { "field mismatch" }) }