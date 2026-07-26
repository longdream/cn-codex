param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$p = Join-Path $Workspace "output/parsed_urls.txt"
if (-not (Test-Path $p)) { return @{ pass = 0; reason = "missing parsed_urls.txt" } }
$urls = @(Get-Content -Encoding UTF8 $p | Where-Object { $_.Trim() -ne "" })
$ok = $urls.Count -ge 3
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "parse urls ok" } else { "too few urls" }) }