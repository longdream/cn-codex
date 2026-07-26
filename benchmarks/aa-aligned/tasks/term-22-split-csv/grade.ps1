param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$d = Join-Path $Workspace "output/split"
$s = Join-Path $Workspace "output/summary.json"
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing output/split" } }
if (-not (Test-Path $s)) { return @{ pass = 0; reason = "missing summary.json" } }
$files = @(Get-ChildItem -Path $d -File | Select-Object -ExpandProperty Name)
$ok = $files.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "split ok" } else { "too few files" }) }