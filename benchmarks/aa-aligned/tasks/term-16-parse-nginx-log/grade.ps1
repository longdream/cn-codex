param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/ip_counts.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing ip_counts.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "nginx parse ok" } else { "bad output" }) }