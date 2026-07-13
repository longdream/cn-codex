param(
    [Parameter(Mandatory = $true)]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [string]$OutDir,

    [string]$MainExe = "",
    [string]$UpdaterExe = "",
    [string]$PortableDir = "",
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

function Resolve-ExistingDir {
    param([string]$Path, [string]$Label)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "$Label not found: $Path"
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

function Ensure-Dir {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        New-Item -ItemType Directory -Path $Path -Force | Out-Null
    }
}

function Should-SkipPortableItem {
    param([string]$Name)
    $n = $Name.ToLowerInvariant()
    return @(
        "update-artifacts",
        "latest.json",
        ".cn-codex-update-work",
        "logs",
        "ebwebview",
        "ebwebview0",
        "ebwebview1"
    ) -contains $n -or $n.StartsWith("ebwebview")
}

$Version = $Version.Trim()
if ([string]::IsNullOrWhiteSpace($Version)) {
    throw "Version is empty"
}

$BaseUrl = $BaseUrl.TrimEnd("/")
if ([string]::IsNullOrWhiteSpace($BaseUrl)) {
    throw "BaseUrl is empty"
}

Ensure-Dir -Path $OutDir
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path

$clientDir = Join-Path $OutDir "client"
$uploadDir = Join-Path $OutDir "update-upload"
$filesDir = Join-Path $uploadDir "files"
$stageDir = Join-Path $OutDir "portable-stage"

foreach ($dir in @($clientDir, $uploadDir, $filesDir)) {
    Ensure-Dir -Path $dir
}

if (Test-Path -LiteralPath $stageDir) {
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}
Ensure-Dir -Path $stageDir

$hasPortableDir = -not [string]::IsNullOrWhiteSpace($PortableDir)
if ($hasPortableDir) {
    $PortableDir = Resolve-ExistingDir -Path $PortableDir -Label "PortableDir"
    Write-Host "[update-artifacts] packaging portable dir: $PortableDir"

    Get-ChildItem -LiteralPath $PortableDir -Force | ForEach-Object {
        if (Should-SkipPortableItem -Name $_.Name) {
            Write-Host "[update-artifacts] skip: $($_.Name)"
            return
        }
        $dest = Join-Path $stageDir $_.Name
        if ($_.PSIsContainer) {
            Copy-Item -LiteralPath $_.FullName -Destination $dest -Recurse -Force
        } else {
            Copy-Item -LiteralPath $_.FullName -Destination $dest -Force
        }
    }
} else {
    if ([string]::IsNullOrWhiteSpace($MainExe)) {
        throw "MainExe is required when PortableDir is not provided"
    }
    $MainExe = Resolve-ExistingFile -Path $MainExe -Label "Main exe"
    Copy-Item -LiteralPath $MainExe -Destination (Join-Path $stageDir "CN-Codex.exe") -Force

    if (-not [string]::IsNullOrWhiteSpace($UpdaterExe)) {
        $UpdaterExe = Resolve-ExistingFile -Path $UpdaterExe -Label "Updater exe"
        Copy-Item -LiteralPath $UpdaterExe -Destination (Join-Path $stageDir "updater.exe") -Force
    }
}

$stageMain = Join-Path $stageDir "CN-Codex.exe"
if (-not (Test-Path -LiteralPath $stageMain -PathType Leaf)) {
    # Accept cn-codex.exe naming and normalize for clients.
    $altMain = Join-Path $stageDir "cn-codex.exe"
    if (Test-Path -LiteralPath $altMain -PathType Leaf) {
        Copy-Item -LiteralPath $altMain -Destination $stageMain -Force
    } else {
        throw "CN-Codex.exe missing in package stage: $stageDir"
    }
}

# Keep a flat client copy for local smoke tests.
Get-ChildItem -LiteralPath $clientDir -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem -LiteralPath $stageDir -Force | ForEach-Object {
    $dest = Join-Path $clientDir $_.Name
    if ($_.PSIsContainer) {
        Copy-Item -LiteralPath $_.FullName -Destination $dest -Recurse -Force
    } else {
        Copy-Item -LiteralPath $_.FullName -Destination $dest -Force
    }
}

$versionedName = "CN-Codex-$Version.zip"
$versionedPath = Join-Path $filesDir $versionedName
if (Test-Path -LiteralPath $versionedPath) {
    Remove-Item -LiteralPath $versionedPath -Force
}

Write-Host "[update-artifacts] creating zip: $versionedPath"
Compress-Archive -Path (Join-Path $stageDir "*") -DestinationPath $versionedPath -CompressionLevel Optimal -Force
if (-not (Test-Path -LiteralPath $versionedPath -PathType Leaf)) {
    throw "Failed to create zip package: $versionedPath"
}

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
    packageType = "zip"
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

# Cleanup staging to keep artifact folder lean.
Remove-Item -LiteralPath $stageDir -Recurse -Force -ErrorAction SilentlyContinue

$zipSize = (Get-Item -LiteralPath $versionedPath).Length
Write-Host "[update-artifacts] version      = $Version"
Write-Host "[update-artifacts] client dir  = $clientDir"
Write-Host "[update-artifacts] upload dir  = $uploadDir"
Write-Host "[update-artifacts] package     = $versionedName ($zipSize bytes)"
Write-Host "[update-artifacts] download url= $downloadUrl"
Write-Host "[update-artifacts] sha256      = $hash"
Write-Host "[update-artifacts] latest.json = $manifestPath"
