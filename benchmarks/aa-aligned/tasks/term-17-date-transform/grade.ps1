param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/iso_dates.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing iso_dates.txt" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "date transform ok" } else { "bad output" }) }