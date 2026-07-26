param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/changes.json"
$d = Join-Path $Workspace "output"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing changes.json" } }
$files = @(Get-ChildItem -Path $d -Filter "*.txt" -File | Select-Object -ExpandProperty Name)
$ok = $files.Count -ge 1
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "replace ok" } else { "no output files" }) }