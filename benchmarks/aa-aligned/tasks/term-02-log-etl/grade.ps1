param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$csvPath = Join-Path $Workspace "output/levels.csv"
$errPath = Join-Path $Workspace "output/errors.txt"
if (-not (Test-Path $csvPath)) { return @{ pass = 0; reason = "missing levels.csv" } }
if (-not (Test-Path $errPath)) { return @{ pass = 0; reason = "missing errors.txt" } }
$csv = Get-Content -Encoding UTF8 $csvPath | Where-Object { $_.Trim() -ne "" }
if ($csv.Count -lt 2 -or $csv[0].Trim().ToLowerInvariant() -ne "level,count") { return @{ pass = 0; reason = "bad csv" } }
$map = @{}
foreach ($line in $csv[1..($csv.Count - 1)]) { $parts = $line.Split(","); $map[$parts[0].Trim().ToUpperInvariant()] = [int]$parts[1] }
$okMap = ($map["DEBUG"] -eq 1) -and ($map["ERROR"] -eq 2) -and ($map["INFO"] -eq 3) -and ($map["WARN"] -eq 1)
$errs = @(Get-Content -Encoding UTF8 $errPath | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne "" })
$okErr = ($errs.Count -eq 2) -and ($errs[0] -eq "disk full") -and ($errs[1] -eq "auth failed")
return @{ pass = $(if ($okMap -and $okErr) { 1 } else { 0 }); reason = $(if ($okMap -and $okErr) { "log etl ok" } else { "csv/err mismatch" }) }