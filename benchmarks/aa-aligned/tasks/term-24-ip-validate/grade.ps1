param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$v = Join-Path $Workspace "output/valid_ips.txt"
$iv = Join-Path $Workspace "output/invalid_ips.txt"
if (-not (Test-Path $v)) { return @{ pass = 0; reason = "missing valid_ips.txt" } }
if (-not (Test-Path $iv)) { return @{ pass = 0; reason = "missing invalid_ips.txt" } }
$valid = @(Get-Content -Encoding UTF8 $v | Where-Object { $_.Trim() -ne "" })
$invalid = @(Get-Content -Encoding UTF8 $iv | Where-Object { $_.Trim() -ne "" })
$ok = $valid.Count -ge 3 -and $invalid.Count -ge 2
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "ip validate ok" } else { "bad classification" }) }