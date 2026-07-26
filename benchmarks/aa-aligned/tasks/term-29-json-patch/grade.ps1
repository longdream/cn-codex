param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/patched.json"
$c = Join-Path $Workspace "output/changelog.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing patched.json" } }
if (-not (Test-Path $c)) { return @{ pass = 0; reason = "missing changelog.txt" } }
return @{ pass = 1; reason = "json patch ok" }