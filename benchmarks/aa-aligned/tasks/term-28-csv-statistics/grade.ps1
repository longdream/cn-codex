param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$j = Join-Path $Workspace "output/stats.json"
$t = Join-Path $Workspace "output/summary.txt"
if (-not (Test-Path $j)) { return @{ pass = 0; reason = "missing stats.json" } }
if (-not (Test-Path $t)) { return @{ pass = 0; reason = "missing summary.txt" } }
return @{ pass = 1; reason = "csv stats ok" }