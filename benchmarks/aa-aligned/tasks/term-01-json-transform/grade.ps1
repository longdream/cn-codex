param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$outJson = Join-Path $Workspace "output/top_active.json"
$outSum = Join-Path $Workspace "output/summary.txt"
if (-not (Test-Path $outJson)) { return @{ pass = 0; reason = "missing top_active.json" } }
if (-not (Test-Path $outSum)) { return @{ pass = 0; reason = "missing summary.txt" } }
try { $arr = Get-Content -Raw -Encoding UTF8 $outJson | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
if ($arr.Count -ne 3) { return @{ pass = 0; reason = "expected 3" } }
$ids = @($arr | ForEach-Object { [int]$_.id })
$ok = ($ids[0] -eq 3 -and $ids[1] -eq 1 -and $ids[2] -eq 5)
$sum = (Get-Content -Raw -Encoding UTF8 $outSum).Trim()
$ok = $ok -and ($sum -eq "count=3;max=99")
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "json transform ok" } else { "bad output" }) }