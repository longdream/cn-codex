param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$v = Join-Path $Workspace "output/valid.txt"
$iv = Join-Path $Workspace "output/invalid.txt"
if (-not (Test-Path $v)) { return @{ pass = 0; reason = "missing valid.txt" } }
if (-not (Test-Path $iv)) { return @{ pass = 0; reason = "missing invalid.txt" } }
$valid = @(Get-Content -Encoding UTF8 $v | Where-Object { $_.Trim() -ne "" })
$invalid = @(Get-Content -Encoding UTF8 $iv | Where-Object { $_.Trim() -ne "" })
$ok = $valid.Count -ge 2 -and $invalid.Count -ge 1
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "validate ok" } else { "bad classification" }) }