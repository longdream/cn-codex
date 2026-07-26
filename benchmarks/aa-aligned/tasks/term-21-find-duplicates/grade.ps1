param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/duplicates.json"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing duplicates.json" } }
try { $d = Get-Content -Raw -Encoding UTF8 $p | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
return @{ pass = 1; reason = "duplicates check ok" }