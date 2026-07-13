param(
  [ValidateSet("normal", "fixed")]
  [string]$Mode = "normal",
  [string]$RuntimeVersionFile = "release/webview2-runtime.version",
  [string]$RuntimeUrlFile = "release/webview2-runtime.url",
  [string]$RuntimeCabUrl = "",
  [string]$RuntimeCabPath = "",
  [string]$PublishDir = "publish",
  [string]$ArtifactPrefix = "CN-Codex-portable-x64",
  [switch]$SkipBuild,
  [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
  Write-Host ""
  Write-Host "==> $Message" -ForegroundColor Cyan
}

function Read-FirstMeaningfulLine([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) {
    return ""
  }
  foreach ($line in Get-Content -LiteralPath $Path) {
    $trimmed = $line.Trim()
    if ($trimmed.Length -eq 0) { continue }
    if ($trimmed.StartsWith("#")) { continue }
    return $trimmed
  }
  return ""
}

function Invoke-CheckedCommand {
  param(
    [Parameter(Mandatory = $true)][string]$Command,
    [Parameter(Mandatory = $true)][string[]]$Arguments,
    [string]$WorkingDirectory = ""
  )

  $display = "$Command $($Arguments -join ' ')"
  if ($DryRun) {
    Write-Host "[dry-run] $display"
    return
  }

  if ($WorkingDirectory) {
    Push-Location $WorkingDirectory
  }
  try {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
      throw "Command failed ($LASTEXITCODE): $display"
    }
  } finally {
    if ($WorkingDirectory) {
      Pop-Location
    }
  }
}

function Compress-ArchiveWithRetry {
  param(
    [Parameter(Mandatory = $true)][string]$SourcePath,
    [Parameter(Mandatory = $true)][string]$DestinationPath,
    [int]$MaxAttempts = 3,
    [int]$RetryDelaySeconds = 2
  )

  for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
    try {
      Compress-Archive -Path $SourcePath -DestinationPath $DestinationPath -CompressionLevel Optimal
      return
    } catch {
      if ($attempt -ge $MaxAttempts) {
        throw
      }
      Write-Warning "Compress-Archive attempt $attempt failed: $($_.Exception.Message). Retrying in $RetryDelaySeconds second(s)..."
      Start-Sleep -Seconds $RetryDelaySeconds
    }
  }
}

function Ensure-Directory {
  param([Parameter(Mandatory = $true)][string]$Path)
  if ($DryRun) {
    Write-Host "[dry-run] would ensure directory $Path"
    return
  }
  New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Copy-DirectoryToParent {
  param(
    [Parameter(Mandatory = $true)][string]$SourceDir,
    [Parameter(Mandatory = $true)][string]$DestinationParent,
    [string]$DisplayName = "",
    [switch]$Required
  )

  $leaf = Split-Path -Path $SourceDir -Leaf
  $targetDir = Join-Path $DestinationParent $leaf
  $label = if ($DisplayName) { $DisplayName } else { $leaf }

  if (-not (Test-Path -LiteralPath $SourceDir)) {
    if ($Required) {
      throw "Required directory does not exist: $SourceDir"
    }
    if ($DryRun) {
      Write-Host "[dry-run] would create empty directory $targetDir (source missing: $SourceDir)"
    } else {
      New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    }
    Write-Host " - $label missing in source, created empty directory"
    return
  }

  if ($DryRun) {
    Write-Host "[dry-run] would copy $SourceDir -> $targetDir"
    return
  }

  Copy-Item -LiteralPath $SourceDir -Destination $DestinationParent -Recurse -Force
  Write-Host " - $label copied"
}

function Bundle-EmbeddedNodeRuntime {
  param(
    [Parameter(Mandatory = $true)][string]$RepoRootPath,
    [Parameter(Mandatory = $true)][string]$CodeyDestinationDir
  )

  $NodeVersion = "22.16.0"
  $NodeArchiveName = "node-v$NodeVersion-win-x64.zip"
  $NodeDownloadUrl = "https://nodejs.org/dist/v$NodeVersion/$NodeArchiveName"
  $NodeCacheDir = Join-Path $RepoRootPath "tools/node-cache"
  $NodeArchivePath = Join-Path $NodeCacheDir $NodeArchiveName
  $NodeTempExtractDir = Join-Path $CodeyDestinationDir "_node_tmp"
  $NodeExtractedDir = Join-Path $NodeTempExtractDir "node-v$NodeVersion-win-x64"
  $NodeDestDir = Join-Path $CodeyDestinationDir "node"
  $NodeEssentialFiles = @("node.exe", "npm", "npm.cmd", "npx", "npx.cmd", "corepack", "corepack.cmd")

  if ($DryRun) {
    Write-Host "[dry-run] would ensure node cache directory $NodeCacheDir"
    Write-Host "[dry-run] would download Node archive if missing: $NodeDownloadUrl"
    Write-Host "[dry-run] would extract $NodeArchivePath and bundle runtime into $NodeDestDir"
    return
  }

  New-Item -ItemType Directory -Path $NodeCacheDir -Force | Out-Null

  if (-not (Test-Path -LiteralPath $NodeArchivePath)) {
    try {
      Invoke-WebRequest -Uri $NodeDownloadUrl -OutFile $NodeArchivePath
      Write-Host " - downloaded Node.js archive: $NodeArchiveName"
    } catch {
      Write-Warning "Failed to download Node.js archive from $NodeDownloadUrl. Embedded node runtime is skipped. Error: $($_.Exception.Message)"
      return
    }
  } else {
    Write-Host " - using cached Node.js archive: $NodeArchivePath"
  }

  if (Test-Path -LiteralPath $NodeTempExtractDir) {
    Remove-Item -LiteralPath $NodeTempExtractDir -Recurse -Force
  }
  if (Test-Path -LiteralPath $NodeDestDir) {
    Remove-Item -LiteralPath $NodeDestDir -Recurse -Force
  }

  try {
    Expand-Archive -LiteralPath $NodeArchivePath -DestinationPath $NodeTempExtractDir -Force
  } catch {
    Write-Warning "Failed to extract Node.js archive $NodeArchivePath. Embedded node runtime is skipped. Error: $($_.Exception.Message)"
    if (Test-Path -LiteralPath $NodeTempExtractDir) {
      Remove-Item -LiteralPath $NodeTempExtractDir -Recurse -Force
    }
    return
  }

  if (-not (Test-Path -LiteralPath $NodeExtractedDir)) {
    Write-Warning "Extracted Node.js folder missing: $NodeExtractedDir. Embedded node runtime is skipped."
    if (Test-Path -LiteralPath $NodeTempExtractDir) {
      Remove-Item -LiteralPath $NodeTempExtractDir -Recurse -Force
    }
    return
  }

  New-Item -ItemType Directory -Path $NodeDestDir -Force | Out-Null

  foreach ($filename in $NodeEssentialFiles) {
    $sourcePath = Join-Path $NodeExtractedDir $filename
    if (Test-Path -LiteralPath $sourcePath) {
      Copy-Item -LiteralPath $sourcePath -Destination $NodeDestDir -Force
    }
  }

  $NodeModulesSource = Join-Path $NodeExtractedDir "node_modules"
  if (Test-Path -LiteralPath $NodeModulesSource) {
    Copy-Item -LiteralPath $NodeModulesSource -Destination $NodeDestDir -Recurse -Force
  }

  if (Test-Path -LiteralPath $NodeTempExtractDir) {
    Remove-Item -LiteralPath $NodeTempExtractDir -Recurse -Force
  }

  Write-Host " - codey/node bundled (Node.js v$NodeVersion)"
}

function Resolve-InstalledWebView2RuntimeDir {
  $candidateRoots = @(
    (Join-Path ${env:ProgramFiles(x86)} "Microsoft\EdgeWebView\Application"),
    (Join-Path $env:ProgramFiles "Microsoft\EdgeWebView\Application")
  ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -Unique

  $runtimeDirs = @()
  foreach ($root in $candidateRoots) {
    $dirs = Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' -and (Test-Path -LiteralPath (Join-Path $_.FullName "msedgewebview2.exe")) }
    $runtimeDirs += $dirs
  }
  if (-not $runtimeDirs) {
    return ""
  }
  $latest = $runtimeDirs |
    Sort-Object { [Version]$_.Name } -Descending |
    Select-Object -First 1
  return $latest.FullName
}

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$AppVersion = & (Join-Path $RepoRoot "scripts/read-app-version.ps1")
if (-not $AppVersion) {
  $AppVersion = "unknown"
}

$IsFixedMode = $Mode -eq "fixed"
$PublishRootDir = Join-Path $RepoRoot $PublishDir
$TempDir = Join-Path $RepoRoot ".tmp/release-portable"
$ExpandedDir = Join-Path $TempDir "expanded"

$VersionFilePath = Join-Path $RepoRoot $RuntimeVersionFile
$RuntimeVersion = ""
$RuntimeFolderName = ""
$RuntimeOutputDir = ""
$CabFile = ""
$NuGetPackageFile = ""
$DefaultRuntimeCabPath = ""
$DefaultNuGetPackageUrl = ""
$RuntimeSourceDir = ""
$RuntimeSourceKind = ""
$IsDefaultNuGetSource = $false

if ($IsFixedMode) {
  $RuntimeVersion = Read-FirstMeaningfulLine $VersionFilePath
  if (-not $RuntimeVersion) {
    throw "WebView2 runtime version file is missing or empty: $VersionFilePath"
  }

  $RuntimeFolderName = "Microsoft.WebView2.FixedVersionRuntime.$RuntimeVersion.x64"
  $RuntimeOutputDir = Join-Path $PublishRootDir "webview2-fixed-runtime\$RuntimeVersion"
  $CabName = "$RuntimeFolderName.cab"
  $NuGetPackageName = "WebView2.Runtime.x64.$RuntimeVersion.nupkg"
  $CabFile = Join-Path $TempDir $CabName
  $NuGetPackageFile = Join-Path $TempDir $NuGetPackageName
  $DefaultRuntimeCabPath = Join-Path $RepoRoot "release/$CabName"
  $DefaultNuGetPackageUrl = "https://www.nuget.org/api/v2/package/WebView2.Runtime.X64/$RuntimeVersion"
}

Write-Step "Preparing workspace"
if (-not $DryRun) {
  if (Test-Path -LiteralPath $PublishRootDir) {
    Remove-Item -LiteralPath $PublishRootDir -Recurse -Force
  }
  if ($IsFixedMode -and (Test-Path -LiteralPath $TempDir)) {
    Remove-Item -LiteralPath $TempDir -Recurse -Force
  }
  New-Item -ItemType Directory -Path $PublishRootDir -Force | Out-Null
  if ($IsFixedMode) {
    New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
    New-Item -ItemType Directory -Path $ExpandedDir -Force | Out-Null
  }
} else {
  if ($IsFixedMode) {
    Write-Host "[dry-run] would recreate $PublishRootDir and $TempDir"
  } else {
    Write-Host "[dry-run] would recreate $PublishRootDir"
  }
}

if ($IsFixedMode) {
  Write-Step "Resolving WebView2 runtime source"
  if (-not $RuntimeCabPath -and -not $RuntimeCabUrl -and (Test-Path -LiteralPath $DefaultRuntimeCabPath)) {
    $RuntimeCabPath = $DefaultRuntimeCabPath
  }

  if (-not $RuntimeCabPath -and -not $RuntimeCabUrl) {
    $urlFilePath = Join-Path $RepoRoot $RuntimeUrlFile
    $RuntimeCabUrl = Read-FirstMeaningfulLine $urlFilePath
  }

  if (-not $RuntimeCabPath -and -not $RuntimeCabUrl) {
    $RuntimeCabUrl = $DefaultNuGetPackageUrl
    $IsDefaultNuGetSource = $true
  }

  if ($RuntimeCabPath) {
    $RuntimeSourceKind = "cab"
    if (-not (Test-Path -LiteralPath $RuntimeCabPath) -and -not $DryRun) {
      throw "Specified RuntimeCabPath does not exist: $RuntimeCabPath"
    }
    if ($RuntimeCabPath -match '\.nupkg$') {
      $RuntimeSourceKind = "nupkg"
    }
    if ($DryRun) {
      $destinationFile = if ($RuntimeSourceKind -eq "nupkg") { $NuGetPackageFile } else { $CabFile }
      Write-Host "[dry-run] would copy package from $RuntimeCabPath to $destinationFile"
    } else {
      if ($RuntimeSourceKind -eq "nupkg") {
        Copy-Item -LiteralPath $RuntimeCabPath -Destination $NuGetPackageFile -Force
      } else {
        Copy-Item -LiteralPath $RuntimeCabPath -Destination $CabFile -Force
      }
    }
  } elseif ($RuntimeCabUrl) {
    $RuntimeSourceKind = if ($RuntimeCabUrl -match '\.nupkg($|\?)' -or $RuntimeCabUrl -match 'api/v2/package/WebView2\.Runtime\.X64/') { "nupkg" } else { "cab" }
    $destinationFile = if ($RuntimeSourceKind -eq "nupkg") { $NuGetPackageFile } else { $CabFile }
    if ($DryRun) {
      Write-Host "[dry-run] would download $RuntimeCabUrl -> $destinationFile"
    } else {
      try {
        Invoke-WebRequest -Uri $RuntimeCabUrl -OutFile $destinationFile
      } catch {
        if ($IsDefaultNuGetSource) {
          Write-Warning "Failed to download default NuGet runtime package from $RuntimeCabUrl, fallback to installed runtime. Error: $($_.Exception.Message)"
          $RuntimeSourceKind = ""
        } else {
          throw
        }
      }
    }
  }

  if (-not $RuntimeSourceKind) {
    $RuntimeSourceDir = Resolve-InstalledWebView2RuntimeDir
    if ($RuntimeSourceDir) {
      $RuntimeSourceKind = "installed-runtime"
      Write-Host "No runtime package source configured; fallback to installed WebView2 runtime: $RuntimeSourceDir"
      $InstalledRuntimeVersion = Split-Path -Path $RuntimeSourceDir -Leaf
      if ($InstalledRuntimeVersion -ne $RuntimeVersion) {
        Write-Warning "Configured runtime version is $RuntimeVersion, but local installed runtime is $InstalledRuntimeVersion. The package will contain $InstalledRuntimeVersion binaries under webview2-fixed-runtime\\$RuntimeVersion."
      }
    } else {
      throw "No WebView2 runtime source found. Provide -RuntimeCabPath, -RuntimeCabUrl, put CAB at $DefaultRuntimeCabPath, fill release/webview2-runtime.url, or install WebView2 runtime on build machine."
    }
  }

  Write-Step "Extracting fixed WebView2 runtime"
  if ($RuntimeSourceKind -eq "cab") {
    Invoke-CheckedCommand -Command "expand.exe" -Arguments @($CabFile, "-F:*", $ExpandedDir)

    if (-not $DryRun) {
      $RuntimeSourceDir = Join-Path $ExpandedDir $RuntimeFolderName
      if (-not (Test-Path -LiteralPath $RuntimeSourceDir)) {
        $RuntimeSourceDir = Get-ChildItem -LiteralPath $ExpandedDir -Directory -Recurse |
          Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "msedgewebview2.exe") } |
          Select-Object -First 1 -ExpandProperty FullName
      }
      if (-not $RuntimeSourceDir) {
        throw "Unable to locate extracted fixed runtime folder in $ExpandedDir"
      }
    } else {
      Write-Host "[dry-run] would extract CAB and copy runtime into $RuntimeOutputDir"
    }
  } elseif ($RuntimeSourceKind -eq "nupkg") {
    if ($DryRun) {
      Write-Host "[dry-run] would extract NUPKG and copy runtime into $RuntimeOutputDir"
    } else {
      Add-Type -AssemblyName System.IO.Compression.FileSystem
      [System.IO.Compression.ZipFile]::ExtractToDirectory($NuGetPackageFile, $ExpandedDir)
      $RuntimeSourceDir = Get-ChildItem -LiteralPath $ExpandedDir -Directory -Recurse |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "msedgewebview2.exe") } |
        Select-Object -First 1 -ExpandProperty FullName
      if (-not $RuntimeSourceDir) {
        throw "Unable to locate runtime folder in extracted NuGet package at $ExpandedDir"
      }
    }
  } elseif ($RuntimeSourceKind -eq "installed-runtime" -and $DryRun) {
    Write-Host "[dry-run] would copy installed runtime from $RuntimeSourceDir into $RuntimeOutputDir"
  }

  if (-not $DryRun) {
    if (-not $RuntimeSourceDir) {
      throw "Runtime source directory was not resolved."
    }
    New-Item -ItemType Directory -Path $RuntimeOutputDir -Force | Out-Null
    Copy-Item -Path (Join-Path $RuntimeSourceDir "*") -Destination $RuntimeOutputDir -Recurse -Force
    if (-not (Test-Path -LiteralPath (Join-Path $RuntimeOutputDir "msedgewebview2.exe"))) {
      throw "Fixed runtime output is invalid: $RuntimeOutputDir"
    }
  }
} else {
  Write-Step "Skipping fixed WebView2 runtime packaging (mode=normal)"
}

if (-not $SkipBuild) {
  Write-Step "Building Tauri release (--no-bundle)"
  Invoke-CheckedCommand -Command "pnpm" -Arguments @("tauri", "build", "--no-bundle") -WorkingDirectory $RepoRoot
} else {
  Write-Step "Skipping build as requested"
}

Write-Step "Collecting release artifacts"
$ReleaseDir = Join-Path $RepoRoot "src-tauri/target/release"
if (-not $DryRun -and -not (Test-Path -LiteralPath (Join-Path $ReleaseDir "CN-Codex.exe"))) {
  throw "CN-Codex.exe not found in $ReleaseDir. Build may have failed."
}

$ArtifactPatterns = if ($IsFixedMode) { @("CN-Codex.exe", "*.dll", "*.pdb") } else { @("CN-Codex.exe", "*.dll") }
foreach ($pattern in $ArtifactPatterns) {
  if ($DryRun) {
    Write-Host "[dry-run] would copy $pattern from $ReleaseDir to $PublishRootDir"
    continue
  }
  Get-ChildItem -LiteralPath $ReleaseDir -File -Filter $pattern -ErrorAction SilentlyContinue |
    Copy-Item -Destination $PublishRootDir -Force
}

Write-Step "Copying updater.exe"
$UpdaterSourcePath = Join-Path $ReleaseDir "updater.exe"
$UpdaterDestPath = Join-Path $PublishRootDir "updater.exe"
if ($DryRun) {
  Write-Host "[dry-run] would copy $UpdaterSourcePath -> $UpdaterDestPath"
} else {
  if (-not (Test-Path -LiteralPath $UpdaterSourcePath)) {
    throw "updater.exe not found in $ReleaseDir. Build may have failed."
  }
  Copy-Item -LiteralPath $UpdaterSourcePath -Destination $UpdaterDestPath -Force
  Write-Host " - updater.exe copied"
}

if ($IsFixedMode) {
  $VersionOutputPath = Join-Path $PublishRootDir "webview2-runtime.version"
  if ($DryRun) {
    Write-Host "[dry-run] would copy $VersionFilePath to $VersionOutputPath"
  } else {
    Copy-Item -LiteralPath $VersionFilePath -Destination $VersionOutputPath -Force
  }
}

Write-Step "Copying OCR runtime resources"
$ResourcesDestDir = Join-Path $PublishRootDir "resources"
$OcrSourceDir = Join-Path $RepoRoot "src-tauri/resources/ocr"
Ensure-Directory -Path $ResourcesDestDir
Copy-DirectoryToParent -SourceDir $OcrSourceDir -DestinationParent $ResourcesDestDir -DisplayName "resources/ocr" -Required

Write-Step "Validating OCR runtime resources"
$OcrDestDir = Join-Path $ResourcesDestDir "ocr"
$RequiredOcrFiles = @(
  "ppocrv5_mobile_det.onnx",
  "ppocrv5_mobile_rec.onnx",
  "ppocrv5_mobile_vocab.txt",
  "onnxruntime.dll",
  "onnxruntime_providers_shared.dll"
)
foreach ($ocrFile in $RequiredOcrFiles) {
  $ocrFilePath = Join-Path $OcrDestDir $ocrFile
  if ($DryRun) {
    Write-Host "[dry-run] would verify OCR file exists: $ocrFilePath"
    continue
  }
  if (-not (Test-Path -LiteralPath $ocrFilePath -PathType Leaf)) {
    throw "Required OCR resource missing in publish output: $ocrFilePath"
  }
}
if (-not $DryRun) {
  Write-Host " - OCR runtime resources verified"
}

Write-Step "Copying workspace runtime resources"
$CodeySourceDir = Join-Path $RepoRoot "codey"
$CodeyDestDir = Join-Path $PublishRootDir "codey"

if (-not (Test-Path -LiteralPath $CodeySourceDir)) {
  throw "Workspace runtime directory not found: $CodeySourceDir"
}

Ensure-Directory -Path $CodeyDestDir

$RequiredSkillFiles = @(
  (Join-Path $CodeySourceDir "skills\agent-reach\SKILL.md"),
  (Join-Path $CodeySourceDir "skills\ponytail\SKILL.md")
)

foreach ($requiredSkill in $RequiredSkillFiles) {
  if (-not (Test-Path -LiteralPath $requiredSkill)) {
    throw "Required skill file not found: $requiredSkill"
  }
}
if ($DryRun) {
  foreach ($requiredSkill in $RequiredSkillFiles) {
    Write-Host "[dry-run] verified required skill file $requiredSkill"
  }
} else {
  Write-Host " - required skills verified: agent-reach, ponytail"
}

Copy-DirectoryToParent -SourceDir (Join-Path $CodeySourceDir "skills") -DestinationParent $CodeyDestDir -DisplayName "codey/skills" -Required
Copy-DirectoryToParent -SourceDir (Join-Path $CodeySourceDir "plugins") -DestinationParent $CodeyDestDir -DisplayName "codey/plugins"
Copy-DirectoryToParent -SourceDir (Join-Path $CodeySourceDir "robots") -DestinationParent $CodeyDestDir -DisplayName "codey/robots"

$CodeyRuntimeDirs = @("sessions", "memories", "browser")
foreach ($runtimeDir in $CodeyRuntimeDirs) {
  Ensure-Directory -Path (Join-Path $CodeyDestDir $runtimeDir)
}

$SensitiveCodeyFiles = @(
  "config.toml",
  "usage.db",
  "usage.db-shm",
  "usage.db-wal",
  "hooks.json",
  "browser\visible-browser.json"
)
foreach ($sensitiveFile in $SensitiveCodeyFiles) {
  $targetFile = Join-Path $CodeyDestDir $sensitiveFile
  if ($DryRun) {
    Write-Host "[dry-run] would delete sensitive file if exists: $targetFile"
    continue
  }
  if (Test-Path -LiteralPath $targetFile) {
    Remove-Item -LiteralPath $targetFile -Force
  }
}

$SensitiveCodeyDirs = @(
  "browser\webview-data",
  "browser\screenshots"
)
foreach ($sensitiveDir in $SensitiveCodeyDirs) {
  $targetDir = Join-Path $CodeyDestDir $sensitiveDir
  if ($DryRun) {
    Write-Host "[dry-run] would delete sensitive directory if exists: $targetDir"
    continue
  }
  if (Test-Path -LiteralPath $targetDir) {
    Remove-Item -LiteralPath $targetDir -Recurse -Force
  }
}

Write-Step "Copying mobile web static files"
$MobileDistSourceDir = Join-Path $RepoRoot "mobile-dist"
$MobileDistDestDir = Join-Path $PublishRootDir "mobile-dist"
if (-not (Test-Path -LiteralPath $MobileDistSourceDir)) {
  throw "mobile-dist folder not found. Build mobile web first (e.g. pnpm --dir mobile-web build). Missing path: $MobileDistSourceDir"
}

if ($DryRun) {
  Write-Host "[dry-run] would copy $MobileDistSourceDir -> $MobileDistDestDir"
} else {
  Copy-Item -LiteralPath $MobileDistSourceDir -Destination $PublishRootDir -Recurse -Force
  if (-not (Test-Path -LiteralPath (Join-Path $MobileDistDestDir "index.html"))) {
    throw "mobile-dist/index.html missing after copy: $MobileDistDestDir"
  }
}

Write-Step "Bundling embedded Node.js runtime"
Bundle-EmbeddedNodeRuntime -RepoRootPath $RepoRoot -CodeyDestinationDir $CodeyDestDir

Write-Step "Generating portable ZIP"
$ArtifactName = if ($IsFixedMode) {
  "$ArtifactPrefix-fixed-webview2-$AppVersion.zip"
} else {
  "$ArtifactPrefix-$AppVersion.zip"
}
$ArtifactPath = Join-Path $RepoRoot $ArtifactName
if ($DryRun) {
  Write-Host "[dry-run] would compress $PublishRootDir -> $ArtifactPath"
} else {
  if (Test-Path -LiteralPath $ArtifactPath) {
    Remove-Item -LiteralPath $ArtifactPath -Force
  }
  Compress-ArchiveWithRetry -SourcePath (Join-Path $PublishRootDir "*") -DestinationPath $ArtifactPath
}

Write-Step "Done"
Write-Host "Mode: $Mode"
Write-Host "App version: $AppVersion"
if ($IsFixedMode) {
  Write-Host "Runtime version: $RuntimeVersion"
}
Write-Host "Publish directory: $PublishRootDir"
Write-Host "ZIP artifact: $ArtifactPath"
