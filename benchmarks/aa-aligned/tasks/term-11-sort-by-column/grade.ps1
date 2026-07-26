param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/sorted.csv"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing sorted.csv" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 5
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "sort ok" } else { "bad output" }) }