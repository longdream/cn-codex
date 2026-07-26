param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/plain_text.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing plain_text.txt" } }
$txt = (Get-Content -Raw -Encoding UTF8 $p).Trim()
$ok = $txt.Length -gt 20
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "html to text ok" } else { "too short" }) }