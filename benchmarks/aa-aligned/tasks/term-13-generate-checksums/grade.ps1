param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/checksums.json"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing checksums.json" } }
try { $d = Get-Content -Raw -Encoding UTF8 $p | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$ok = ($d.PSObject.Properties.Name | Measure-Object).Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "checksums ok" } else { "too few files" }) }