param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/merged.json"
$s = Join-Path $Workspace "output/stats.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing merged.json" } }
if (-not (Test-Path $s)) { return @{ pass = 0; reason = "missing stats.txt" } }
try { $d = Get-Content -Raw -Encoding UTF8 $p | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$cnt = ($d | Measure-Object).Count
$ok = $cnt -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "merge ok" } else { "too few records" }) }