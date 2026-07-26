param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$d = Join-Path $Workspace "output/diff.txt"
$c = Join-Path $Workspace "output/common.txt"
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing diff.txt" } }
if (-not (Test-Path $c)) { return @{ pass = 0; reason = "missing common.txt" } }
return @{ pass = 1; reason = "diff ok" }