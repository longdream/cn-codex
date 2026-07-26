param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$e = Join-Path $Workspace "output/encrypted.bin"
$d = Join-Path $Workspace "output/decrypted.txt"
if (-not (Test-Path $e)) { return @{ pass = 0; reason = "missing encrypted.bin" } }
if (-not (Test-Path $d)) { return @{ pass = 0; reason = "missing decrypted.txt" } }
return @{ pass = 1; reason = "encrypt decrypt ok" }