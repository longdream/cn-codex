param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/high_value.csv"
$t = Join-Path $Workspace "output/total.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing high_value.csv" } }
if (-not (Test-Path $t)) { return @{ pass = 0; reason = "missing total.txt" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "filter ok" } else { "bad output" }) }