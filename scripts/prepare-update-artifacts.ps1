param(
    [Parameter(Mandatory = $true)]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [string]$MainExe,

    [Parameter(Mandatory = $true)]
    [string]$OutDir,

    [string]$UpdaterExe = "",
    [string]$BaseUrl = "http://47.113.221.244:5005",
    [string]$Notes = "",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

function Resolve-ExistingFile {
    param([string]$Path, [string]$Label)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label not found: $Path"
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

$Version = $Version.Trim()
if ([string]::IsNullOrWhiteSpace($Version)) {
    throw "Version is empty"
}

$MainExe = Resolve-ExistingFile -Path $MainExe -Label "Main exe"
$BaseUrl = $BaseUrl.TrimEnd("/")
if ([string]::IsNullOrWhiteSpace($BaseUrl)) {
    throw "BaseUrl is empty"
}

if (-not (Test-Path -LiteralPath $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path

$clientDir = Join-Path $OutDir "client"
$uploadDir = Join-Path $OutDir "update-upload"
$filesDir = Join-Path $uploadDir "files"

foreach ($dir in @($clientDir, $uploadDir, $filesDir)) {
    if (-not (Test-Path -LiteralPath $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
}

$clientMain = Join-Path $clientDir "CN-Codex.exe"
Copy-Item -LiteralPath $MainExe -Destination $clientMain -Force

if (-not [string]::IsNullOrWhiteSpace($UpdaterExe)) {
    $UpdaterExe = Resolve-ExistingFile -Path $UpdaterExe -Label "Updater exe"
    Copy-Item -LiteralPath $UpdaterExe -Destination (Join-Path $clientDir "updater.exe") -Force
}

$versionedName = "CN-Codex-$Version.exe"
$versionedPath = Join-Path $filesDir $versionedName
Copy-Item -LiteralPath $MainExe -Destination $versionedPath -Force

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $versionedPath).Hash.ToLowerInvariant()
$downloadUrl = "$BaseUrl/files/$versionedName"
$publishedAt = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
if ([string]::IsNullOrWhiteSpace($Notes)) {
    $Notes = "CN-Codex $Version"
}

$manifest = [ordered]@{
    version     = $Version
    url         = $downloadUrl
    sha256      = $hash
    notes       = $Notes
    force       = [bool]$Force
    publishedAt = $publishedAt
}

$manifestJson = $manifest | ConvertTo-Json -Depth 4
$manifestPath = Join-Path $uploadDir "latest.json"
$clientManifestPath = Join-Path $clientDir "latest.json"
$rootManifestPath = Join-Path $OutDir "latest.json"

# UTF-8 without BOM for server/public compatibility
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($manifestPath, $manifestJson + [Environment]::NewLine, $utf8NoBom)
[System.IO.File]::WriteAllText($clientManifestPath, $manifestJson + [Environment]::NewLine, $utf8NoBom)
[System.IO.File]::WriteAllText($rootManifestPath, $manifestJson + [Environment]::NewLine, $utf8NoBom)

Write-Host "[update-artifacts] version      = $Version"
Write-Host "[update-artifacts] client dir  = $clientDir"
Write-Host "[update-artifacts] upload dir  = $uploadDir"
Write-Host "[update-artifacts] download url= $downloadUrl"
Write-Host "[update-artifacts] sha256      = $hash"
Write-Host "[update-artifacts] latest.json = $manifestPath"
