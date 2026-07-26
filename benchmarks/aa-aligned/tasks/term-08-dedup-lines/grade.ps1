param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/unique.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing unique.txt" } }
$lines = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $lines.Count -ge 4
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "dedup ok" } else { "too few lines" }) }