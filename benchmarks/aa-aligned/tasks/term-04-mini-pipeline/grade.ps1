param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$csvPath = Join-Path $Workspace "output/by_region.csv"
$totPath = Join-Path $Workspace "output/total.json"
if (-not (Test-Path $csvPath)) { return @{ pass = 0; reason = "missing by_region.csv" } }
if (-not (Test-Path $totPath)) { return @{ pass = 0; reason = "missing total.json" } }
$lines = @(Get-Content -Encoding UTF8 $csvPath | Where-Object { $_.Trim() -ne "" })
if ($lines.Count -ne 3) { return @{ pass = 0; reason = "expected header + 2 regions" } }
if ($lines[0].Trim().ToLowerInvariant() -ne "region,orders,gross,net") { return @{ pass = 0; reason = "bad header" } }
function Parse-Row([string]$line) { $p = $line.Split(","); return [pscustomobject]@{ region = $p[0].Trim().ToLowerInvariant(); orders = [int]$p[1]; gross = [decimal]$p[2]; net = [decimal]$p[3] } }
$r1 = Parse-Row $lines[1]; $r2 = Parse-Row $lines[2]
$okCsv = ($r1.region -eq "east" -and $r1.orders -eq 2 -and [math]::Round($r1.gross, 2) -eq 30.00 -and [math]::Round($r1.net, 2) -eq 27.00) -and
  ($r2.region -eq "west" -and $r2.orders -eq 1 -and [math]::Round($r2.gross, 2) -eq 25.00 -and [math]::Round($r2.net, 2) -eq 22.75)
try { $tot = Get-Content -Raw -Encoding UTF8 $totPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid total.json" } }
$okTot = ([int]$tot.orders -eq 3) -and ([math]::Round([decimal]$tot.gross, 2) -eq 55.00) -and ([math]::Round([decimal]$tot.net, 2) -eq 49.75)
return @{ pass = $(if ($okCsv -and $okTot) { 1 } else { 0 }); reason = $(if ($okCsv -and $okTot) { "pipeline ok" } else { "aggregation mismatch" }) }