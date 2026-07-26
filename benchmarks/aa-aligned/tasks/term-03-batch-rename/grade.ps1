param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
$manPath = Join-Path $Workspace "output/manifest.json"
$outDir = Join-Path $Workspace "output/renamed"
if (-not (Test-Path $manPath)) { return @{ pass = 0; reason = "missing manifest.json" } }
if (-not (Test-Path $outDir)) { return @{ pass = 0; reason = "missing output/renamed" } }
try { $man = Get-Content -Raw -Encoding UTF8 $manPath | ConvertFrom-Json } catch { return @{ pass = 0; reason = "invalid json" } }
$files = @($man.files)
if ($files.Count -ne 3) { return @{ pass = 0; reason = "expected 3 files" } }
$tos = @($files | ForEach-Object { [string]$_.to })
$expectedTo = @("hello_world.txt", "note_a.txt", "report_final.txt")
$ok = $true
for ($i = 0; $i -lt 3; $i++) { if ($tos[$i] -ne $expectedTo[$i]) { $ok = $false } }
$expectedContent = @{ "note_a.txt" = "alpha"; "hello_world.txt" = "beta"; "report_final.txt" = "gamma" }
foreach ($name in $expectedContent.Keys) {
  $p = Join-Path $outDir $name
  if (-not (Test-Path $p)) { $ok = $false; continue }
  $c = (Get-Content -Raw -Encoding UTF8 $p).Trim()
  if ($c -ne $expectedContent[$name]) { $ok = $false }
}
return @{ pass = $(if ($ok) { 1 } else { 0 }); reason = $(if ($ok) { "batch rename ok" } else { "mismatch" }) }