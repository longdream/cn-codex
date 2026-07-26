param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$d = Join-Path $Workspace "output/encoded"
$m = Join-Path $Workspace "output/manifest.json"
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing output/encoded" } }
if (-not (Test-Path $m)) { return @{ pass = 0; reason = "missing manifest.json" } }
$files = @(Get-ChildItem -Path $d -File | Select-Object -ExpandProperty Name)
$ok = $files.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "base64 encode ok" } else { "too few files" }) }