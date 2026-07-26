param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$a = Join-Path $Workspace "output/archive.zip"
$e = Join-Path $Workspace "output/extracted"
$alt = Join-Path $Workspace "output/archive.tar.gz"
if (-not (Test-Path $a) -and -not (Test-Path $alt)) { return @{ pass = 0; reason = "missing archive" } }
if (-not (Test-Path $e)) { return @{ pass = 0; reason = "missing output/extracted" } }
return @{ pass = 1; reason = "archive ok" }