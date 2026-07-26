param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$answerPath = Join-Path $Workspace "answer.json"
if (-not (Test-Path $answerPath)) { return @{ pass = 0; reason = "missing answer.json" } }
try { $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
# Accept any answer that has at least 4 fields
$fields = @($ans.PSObject.Properties.Name)
if ($fields.Count -ge 4) { return @{ pass = 1; reason = "accepted" } }
return @{ pass = 0; reason = "too few fields" }