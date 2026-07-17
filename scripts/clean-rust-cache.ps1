# Clean Rust/Cargo cache for CN-Codex builds.
# Default mode keeps release deps for faster rebuilds and only removes bulky
# incremental / stale debug artifacts that commonly explode disk usage.
param(
    [ValidateSet("auto", "incremental", "debug", "full")]
    [string]$Mode = "auto",
    [switch]$WhatIf
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$TargetRoot = Join-Path $ProjectRoot "src-tauri\target"

function Get-PathSizeBytes {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) { return 0 }
    return [int64]$sum
}

function Format-Size {
    param([int64]$Bytes)
    if ($Bytes -ge 1GB) { return ("{0:N2} GB" -f ($Bytes / 1GB)) }
    if ($Bytes -ge 1MB) { return ("{0:N1} MB" -f ($Bytes / 1MB)) }
    return ("{0:N0} KB" -f ($Bytes / 1KB))
}

function Remove-PathSafe {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    $size = Get-PathSizeBytes -Path $Path
    if ($WhatIf) {
        Write-Host ("[WhatIf] would remove {0} ({1})" -f $Path, (Format-Size $size))
        return $size
    }
    try {
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
        Write-Host ("[OK] removed {0} ({1})" -f $Path, (Format-Size $size))
        return $size
    } catch {
        Write-Host ("[WARN] failed to remove {0}: {1}" -f $Path, $_.Exception.Message)
        return 0
    }
}

function Get-DriveFreeBytes {
    param([string]$Path)
    $full = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetPathRoot($full)
    if ([string]::IsNullOrWhiteSpace($root)) { return 0 }
    $driveLetter = $root.TrimEnd('\', '/').TrimEnd(':')
    $drive = Get-CimInstance Win32_LogicalDisk -Filter ("DeviceID='{0}:'" -f $driveLetter) -ErrorAction SilentlyContinue
    if ($null -eq $drive) { return 0 }
    return [int64]$drive.FreeSpace
}

if (-not (Test-Path -LiteralPath $TargetRoot)) {
    Write-Host "target not found: $TargetRoot"
    exit 0
}

$beforeTarget = Get-PathSizeBytes -Path $TargetRoot
$beforeFree = Get-DriveFreeBytes -Path $TargetRoot
Write-Host ("target before: {0}" -f (Format-Size $beforeTarget))
Write-Host ("drive free before: {0}" -f (Format-Size $beforeFree))

if ($Mode -eq "auto") {
    # If free space is tight or target is huge, prefer more aggressive cleanup.
    if (($beforeFree -lt 20GB) -or ($beforeTarget -gt 20GB)) {
        $Mode = "debug"
        Write-Host "auto mode selected: debug (tight disk or large target)"
    } else {
        $Mode = "incremental"
        Write-Host "auto mode selected: incremental"
    }
}

$removed = [int64]0
$paths = @()

switch ($Mode) {
    "incremental" {
        $paths += (Join-Path $TargetRoot "release\incremental")
        $paths += (Join-Path $TargetRoot "debug\incremental")
    }
    "debug" {
        $paths += (Join-Path $TargetRoot "debug")
        $paths += (Join-Path $TargetRoot "release\incremental")
    }
    "full" {
        $paths += $TargetRoot
    }
}

foreach ($p in $paths) {
    $removed += Remove-PathSafe -Path $p
}

$afterTarget = Get-PathSizeBytes -Path $TargetRoot
$afterFree = Get-DriveFreeBytes -Path $TargetRoot

Write-Host ""
Write-Host ("mode: {0}" -f $Mode)
Write-Host ("removed approx: {0}" -f (Format-Size $removed))
Write-Host ("target after: {0}" -f (Format-Size $afterTarget))
Write-Host ("drive free after: {0}" -f (Format-Size $afterFree))

if (-not $WhatIf -and $Mode -ne "full" -and (Test-Path -LiteralPath (Join-Path $TargetRoot "release"))) {
    Write-Host "kept release deps/build cache for faster next compile"
}
