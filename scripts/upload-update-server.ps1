# CN-Codex update-server SSH uploader.
# Uploads the update-upload package (latest.json + files/*.zip) produced by
# publish.bat / build.bat to the remote update server over SSH (SFTP via pscp).
#
# Transfer tools (pscp.exe / plink.exe) are auto-downloaded into -ToolsDir on
# first run and cached. No PuTTY installation required.
#
# Credentials can be overridden via environment variables:
#   DEPLOY_HOST, DEPLOY_PORT, DEPLOY_USER, DEPLOY_PASSWORD, DEPLOY_REMOTE_DIR
#
# NOTE: The default password below is stored in plain text per user request.
#       For better security, switch to SSH key auth and remove the password.

param(
    [Parameter(Mandatory = $true)]
    [string]$UploadDir,

    [string]$ToolsDir = "",
    [int]$Port = $(if ($env:DEPLOY_PORT) { [int]$env:DEPLOY_PORT } else { 22 }),
    [string]$RemoteHost = $(if ($env:DEPLOY_HOST) { $env:DEPLOY_HOST } else { "47.113.221.244" }),
    [string]$User = $(if ($env:DEPLOY_USER) { $env:DEPLOY_USER } else { "root" }),
    [string]$Password = $(if ($env:DEPLOY_PASSWORD) { $env:DEPLOY_PASSWORD } else { "49718751L!abcd" }),
    [string]$RemoteDir = $(if ($env:DEPLOY_REMOTE_DIR) { $env:DEPLOY_REMOTE_DIR } else { "/opt/cn-codex-update/public" }),
    [string]$BaseUrl = $(if ($env:UPDATE_BASE_URL) { $env:UPDATE_BASE_URL } else { "http://47.113.221.244:5005" }),
    [switch]$SkipVerify,
    [switch]$SkipHttpCheck
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch { }

function Write-Step([string]$Message) { Write-Host "[upload] $Message" }
function Write-Ok([string]$Message)   { Write-Host "  OK: $Message" -ForegroundColor Green }
function Write-Fail([string]$Message) { Write-Host "  FAIL: $Message" -ForegroundColor Red }

# Run a native command with stderr redirect safe under ErrorActionPreference=Stop.
# Returns the process exit code; console output is swallowed.
function Invoke-Native {
    param([scriptblock]$Block)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $null = & $Block 2>&1
        return $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
    }
}

# Run a native command and return its combined output lines.
function Invoke-NativeOutput {
    param([scriptblock]$Block)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $out = & $Block 2>&1
        $code = $LASTEXITCODE
        return , @($code, @($out | ForEach-Object { "$_" }))
    } finally {
        $ErrorActionPreference = $prev
    }
}

Write-Host "============================================="
Write-Host "  CN-Codex update-server SSH upload"
Write-Host "============================================="

try {
    # ---------------- payload ----------------
    $RemoteDir = $RemoteDir.Trim().TrimEnd("/")
    if ([string]::IsNullOrWhiteSpace($RemoteDir)) { throw "RemoteDir is empty" }
    $remoteFilesDir = "$RemoteDir/files"

    if (-not (Test-Path -LiteralPath $UploadDir -PathType Container)) {
        throw "UploadDir not found: $UploadDir"
    }
    $UploadDir = (Resolve-Path -LiteralPath $UploadDir).Path
    $latestLocal = Join-Path $UploadDir "latest.json"
    if (-not (Test-Path -LiteralPath $latestLocal -PathType Leaf)) {
        throw "latest.json not found in $UploadDir (run the build first)"
    }
    $filesDirLocal = Join-Path $UploadDir "files"
    $zips = @(Get-ChildItem -LiteralPath $filesDirLocal -Filter "*.zip" -File -ErrorAction SilentlyContinue)
    if ($zips.Count -eq 0) {
        throw "No zip package found under $filesDirLocal"
    }

    $version = $null; $packageUrl = $null
    try {
        $manifest = Get-Content -LiteralPath $latestLocal -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($manifest.version) { $version = [string]$manifest.version }
        if ($manifest.url)     { $packageUrl = [string]$manifest.url }
    } catch { }

    Write-Step "Target    : $User@$RemoteHost`:$Port$RemoteDir"
    if ($version) { Write-Step "Version   : $version" }
    Write-Step "Payload   : latest.json + $($zips.Count) zip package(s)"

    # ---------------- transfer tools (pscp + plink) ----------------
    if ([string]::IsNullOrWhiteSpace($ToolsDir)) {
        $ToolsDir = Join-Path $PSScriptRoot "..\tools"
    }
    $ToolsDir = (New-Item -ItemType Directory -Path $ToolsDir -Force).FullName
    $pscpPath  = Join-Path $ToolsDir "pscp.exe"
    $plinkPath = Join-Path $ToolsDir "plink.exe"

    $mirrorBases = @(
        "https://the.earth.li/~sgtatham/putty/latest/w64",
        "https://chiark.greenend.org.uk/~sgtatham/putty/latest/w64"
    )
    foreach ($tool in @(@("pscp.exe", $pscpPath), @("plink.exe", $plinkPath))) {
        $name = $tool[0]
        $dest = $tool[1]
        if (Test-Path -LiteralPath $dest -PathType Leaf) {
            Write-Step "Using cached $name"
            continue
        }
        $downloaded = $false
        foreach ($base in $mirrorBases) {
            try {
                Write-Step "Downloading $name from $base ..."
                Invoke-WebRequest -Uri "$base/$name" -OutFile $dest -UseBasicParsing -TimeoutSec 90
                if ((Test-Path -LiteralPath $dest -PathType Leaf) -and ((Get-Item -LiteralPath $dest).Length -gt 100KB)) {
                    $downloaded = $true
                    break
                }
            } catch {
                Write-Host "  mirror failed: $base ($($_.Exception.Message))" -ForegroundColor Yellow
            }
        }
        if (-not $downloaded) {
            throw "Failed to obtain $name. Check network access or download it manually into $ToolsDir"
        }
    }

    $target = "$User@$RemoteHost"

    # ---------------- host key pinning ----------------
    # Pin the server host key explicitly (SHA256 fingerprint). This avoids the
    # interactive "cache host key?" prompt entirely and survives registry resets.
    # Override with env DEPLOY_HOST_KEY when the server key changes.
    $hostKey = $(if ($env:DEPLOY_HOST_KEY) { $env:DEPLOY_HOST_KEY } else { "SHA256:Wh9ZmWOxuaBvV0PWqzoNlxzWASkRe1b33w/PmrpIwhY" })
    $plinkArgs = @("-ssh", "-batch", "-hostkey", $hostKey, "-P", "$Port", "-pw", $Password)
    $pscpArgs  = @("-batch", "-hostkey", $hostKey, "-P", "$Port", "-pw", $Password)

    # ---------------- connect ----------------
    Write-Step "Connecting to $target ..."
    $probe = Invoke-Native { & $plinkPath @plinkArgs $target exit }
    if ($probe -ne 0) {
        throw "SSH connectivity test failed (exit $probe). Check host/port/username/password."
    }
    Write-Ok "SSH connection established: $target`:$Port"

    # ---------------- remote dir ----------------
    Write-Step "Ensuring remote directory: $remoteFilesDir"
    $mk = Invoke-Native { & $plinkPath @plinkArgs $target "mkdir -p '$remoteFilesDir'" }
    if ($mk -ne 0) { throw "mkdir -p failed on remote server (exit $mk)" }
    Write-Ok "remote dir ready: $remoteFilesDir"

    # ---------------- upload ----------------
    Write-Step "Uploading latest.json ..."
    $rc = Invoke-Native { & $pscpPath @pscpArgs $latestLocal "${target}:$RemoteDir/latest.json" }
    if ($rc -ne 0) { throw "pscp upload failed for latest.json (exit $rc)" }
    Write-Ok "latest.json -> $RemoteDir/latest.json"

    foreach ($zip in $zips) {
        Write-Step ("Uploading {0} ({1:N1} MB) ..." -f $zip.Name, ($zip.Length / 1MB))
        $rc = Invoke-Native { & $pscpPath @pscpArgs $zip.FullName "${target}:$remoteFilesDir/$($zip.Name)" }
        if ($rc -ne 0) { throw "pscp upload failed for $($zip.Name) (exit $rc)" }
        Write-Ok "$($zip.Name) -> $remoteFilesDir/$($zip.Name)"
    }

    # ---------------- verify ----------------
    if (-not $SkipVerify) {
        Write-Step "Verifying uploads (remote sha256sum)..."
        # NOTE: leading comma keeps each item as a nested array (PowerShell
        # @() flattens nested arrays otherwise).
        $verifyItems = , @("latest.json", $latestLocal, "$RemoteDir/latest.json")
        foreach ($zip in $zips) {
            $verifyItems += , @($zip.Name, $zip.FullName, "$remoteFilesDir/$($zip.Name)")
        }
        foreach ($item in $verifyItems) {
            $name       = $item[0]
            $localPath  = $item[1]
            $remotePath = $item[2]
            $localHash  = (Get-FileHash -Algorithm SHA256 -LiteralPath $localPath).Hash.ToLowerInvariant()
            $res = Invoke-NativeOutput { & $plinkPath @plinkArgs $target "sha256sum '$remotePath'" }
            if ($res[0] -ne 0) { throw "remote sha256sum failed for $name (exit $($res[0]))" }
            $hashLine = $res[1] | Where-Object { $_ -match '^[0-9a-fA-F]{64}\s' } | Select-Object -First 1
            if (-not $hashLine) { throw "unexpected sha256sum output for $name" }
            $remoteHash = $hashLine.Split(" ", [System.StringSplitOptions]::RemoveEmptyEntries)[0].ToLowerInvariant()
            if ($remoteHash -ne $localHash) {
                throw "$name hash mismatch: local=$localHash remote=$remoteHash"
            }
            Write-Ok "$name sha256 verified"
        }
    }

    # ---------------- public HTTP smoke check ----------------
    if (-not $SkipHttpCheck) {
        $BaseUrl = $BaseUrl.TrimEnd("/")
        try {
            Write-Step "Checking public manifest: $BaseUrl/latest.json"
            $resp = Invoke-WebRequest -Uri "$BaseUrl/latest.json" -UseBasicParsing -TimeoutSec 15
            if ($resp.StatusCode -eq 200) {
                Write-Ok "public manifest reachable: $BaseUrl/latest.json"
            }
        } catch {
            Write-Host "  WARN: HTTP check failed: $($_.Exception.Message)" -ForegroundColor Yellow
            Write-Host "  (files are uploaded; the web service may need a moment or serve another port)"
        }
    }

    Write-Host ""
    Write-Host "[upload] Deployment complete!" -ForegroundColor Green
    Write-Host "  remote dir : $RemoteDir"
    if ($version)    { Write-Host "  version    : $version" }
    if ($packageUrl) { Write-Host "  package url: $packageUrl" }
    exit 0
}
catch {
    Write-Fail $_.Exception.Message
    Write-Host ""
    Write-Host "Manual fallback:" -ForegroundColor Yellow
    Write-Host ("  scp -r ""{0}\*"" {1}@{2}:{3}/" -f $UploadDir, $User, $RemoteHost, $RemoteDir)
    exit 1
}
