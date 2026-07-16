param(
  [string]$Version = "14.1.1",
  [string]$RepoRoot = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $RepoRoot) {
  $RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
}

$CacheDir = Join-Path $RepoRoot "tools/rg-cache"
$DestDir = Join-Path $RepoRoot "src-tauri/resources/rg"
$ArchiveName = "ripgrep-$Version-x86_64-pc-windows-msvc.zip"
$DownloadUrl = "https://github.com/BurntSushi/ripgrep/releases/download/$Version/$ArchiveName"
$ArchivePath = Join-Path $CacheDir $ArchiveName
$ExtractDir = Join-Path $CacheDir "extract-$Version"
$DestExe = Join-Path $DestDir "rg.exe"

New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null
New-Item -ItemType Directory -Force -Path $DestDir | Out-Null

if (-not (Test-Path -LiteralPath $ArchivePath)) {
  Write-Host "Downloading $DownloadUrl"
  Invoke-WebRequest -Uri $DownloadUrl -OutFile $ArchivePath
} else {
  Write-Host "Using cached archive: $ArchivePath"
}

if (Test-Path -LiteralPath $ExtractDir) {
  Remove-Item -LiteralPath $ExtractDir -Recurse -Force
}
Expand-Archive -LiteralPath $ArchivePath -DestinationPath $ExtractDir -Force

$Found = Get-ChildItem -LiteralPath $ExtractDir -Recurse -Filter "rg.exe" |
  Select-Object -First 1
if (-not $Found) {
  throw "rg.exe not found inside $ArchiveName"
}

Copy-Item -LiteralPath $Found.FullName -Destination $DestExe -Force
Write-Host "Installed bundled ripgrep: $DestExe"
Write-Host ("Size: {0:N1} MB" -f ((Get-Item -LiteralPath $DestExe).Length / 1MB))
