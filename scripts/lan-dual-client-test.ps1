#Requires -Version 5.1
<#
.SYNOPSIS
  Launch two isolated CN-Codex instances on one PC for LAN collab testing.

.DESCRIPTION
  - Isolates node identity/config/knowledge via CN_CODEX_PROJECT_ROOT
  - Isolates WebView2 profile via WEBVIEW2_USER_DATA_FOLDER
  - App auto-picks free listen ports in 47800-47820

.EXAMPLE
  .\scripts\lan-dual-client-test.ps1

.EXAMPLE
  .\scripts\lan-dual-client-test.ps1 -ExePath "D:\apps\cn-codex.exe" -Clean
#>
[CmdletBinding()]
param(
  [string]$ExePath,
  [string]$TestRoot,
  [switch]$PrepareOnly,
  [switch]$Clean
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
  Write-Host ""
  Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Info([string]$Message) {
  Write-Host "    $Message"
}

function Resolve-RepoRoot {
  $scriptDir = Split-Path -Parent $PSCommandPath
  return (Resolve-Path (Join-Path $scriptDir "..")).Path
}

function Find-CnCodexExe([string]$RepoRoot, [string]$Preferred) {
  if ($Preferred) {
    if (-not (Test-Path -LiteralPath $Preferred)) {
      throw "ExePath not found: $Preferred"
    }
    return (Resolve-Path -LiteralPath $Preferred).Path
  }

  $candidates = @(
    (Join-Path $RepoRoot "src-tauri\target\release\cn-codex.exe"),
    (Join-Path $RepoRoot "src-tauri\target\debug\cn-codex.exe"),
    (Join-Path $RepoRoot "publish\cn-codex.exe"),
    (Join-Path $RepoRoot "release\cn-codex.exe")
  )

  foreach ($path in $candidates) {
    if (Test-Path -LiteralPath $path) {
      return (Resolve-Path -LiteralPath $path).Path
    }
  }

  $publishRoot = Join-Path $RepoRoot "publish"
  if (Test-Path -LiteralPath $publishRoot) {
    $found = Get-ChildItem -Path $publishRoot -Filter "cn-codex.exe" -Recurse -ErrorAction SilentlyContinue |
      Select-Object -First 1
    if ($found) {
      return $found.FullName
    }
  }

  throw @"
cn-codex.exe not found.

Build first, or pass -ExePath:
  npm run tauri build
  cargo build --manifest-path src-tauri/Cargo.toml --release
"@
}

function Ensure-Dir([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
  }
}

function Initialize-NodeWorkspace {
  param(
    [string]$NodeRoot,
    [string]$DisplayHint,
    [switch]$WithSampleKnowledge
  )

  Ensure-Dir $NodeRoot
  $codey = Join-Path $NodeRoot "codey"
  $kbDocs = Join-Path $codey "memories\knowledge\docs"
  $kbIndexDir = Join-Path $codey "memories\knowledge"
  Ensure-Dir $kbDocs
  Ensure-Dir (Join-Path $codey "lan_collab")
  Ensure-Dir (Join-Path $codey "skills")
  Ensure-Dir (Join-Path $codey "plugins")

  $configPath = Join-Path $codey "config.toml"
  if (-not (Test-Path -LiteralPath $configPath)) {
    @(
      "# Auto-generated for LAN dual-client test ($DisplayHint)",
      "# Configure providers in UI if you need model-share tests.",
      ""
    ) | Set-Content -LiteralPath $configPath -Encoding UTF8
  }

  if ($WithSampleKnowledge) {
    $docId = "doc_lan_alpha"
    $docPath = Join-Path $kbDocs "$docId.md"
    if (-not (Test-Path -LiteralPath $docPath)) {
      @(
        "# Alpha Doc",
        "",
        "This is sample knowledge for LAN collab dual-client testing.",
        "",
        "Keywords: lan collab alpha secret knowledge",
        ""
      ) | Set-Content -LiteralPath $docPath -Encoding UTF8
    }

    $indexPath = Join-Path $kbIndexDir "index.json"
    if (-not (Test-Path -LiteralPath $indexPath)) {
      $index = @{
        version = 1
        documents = @(
          @{
            docId = $docId
            title = "Alpha Doc"
            path = "docs/$docId.md"
          }
        )
      }
      $index | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $indexPath -Encoding UTF8
    }
  }
}

function Start-IsolatedInstance {
  param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$ProjectRoot,
    [Parameter(Mandatory = $true)][string]$WebViewData,
    [Parameter(Mandatory = $true)][string]$Title
  )

  Ensure-Dir $ProjectRoot
  Ensure-Dir $WebViewData

  # Inject env vars then start. Compatible with Windows PowerShell 5.1.
  $cmd = "set `"CN_CODEX_PROJECT_ROOT=$ProjectRoot`" && set `"WEBVIEW2_USER_DATA_FOLDER=$WebViewData`" && start `"`" `"$Exe`""
  Write-Info "Starting $Title"
  Write-Info "  CN_CODEX_PROJECT_ROOT=$ProjectRoot"
  Write-Info "  WEBVIEW2_USER_DATA_FOLDER=$WebViewData"
  Start-Process -FilePath "cmd.exe" -ArgumentList @("/c", $cmd) | Out-Null
}

$repoRoot = Resolve-RepoRoot
if (-not $TestRoot) {
  $TestRoot = Join-Path $repoRoot ".lan-test"
}

Write-Step "Repo root"
Write-Info $repoRoot

if ($Clean -and (Test-Path -LiteralPath $TestRoot)) {
  Write-Step "Cleaning old test root"
  Write-Info $TestRoot
  Remove-Item -LiteralPath $TestRoot -Recurse -Force
}

$nodeA = Join-Path $TestRoot "node-a"
$nodeB = Join-Path $TestRoot "node-b"
$wvA = Join-Path $TestRoot "webview-a"
$wvB = Join-Path $TestRoot "webview-b"

Write-Step "Preparing isolated workspaces"
Initialize-NodeWorkspace -NodeRoot $nodeA -DisplayHint "OwnerA" -WithSampleKnowledge
Initialize-NodeWorkspace -NodeRoot $nodeB -DisplayHint "MemberB"
Ensure-Dir $wvA
Ensure-Dir $wvB
Write-Info "A: $nodeA"
Write-Info "B: $nodeB"

if ($PrepareOnly) {
  Write-Step "Prepare-only done"
  Write-Info "Start two processes with these env vars:"
  Write-Host ""
  Write-Host "A:" -ForegroundColor Green
  Write-Host "  `$env:CN_CODEX_PROJECT_ROOT = '$nodeA'"
  Write-Host "  `$env:WEBVIEW2_USER_DATA_FOLDER = '$wvA'"
  Write-Host "B:" -ForegroundColor Green
  Write-Host "  `$env:CN_CODEX_PROJECT_ROOT = '$nodeB'"
  Write-Host "  `$env:WEBVIEW2_USER_DATA_FOLDER = '$wvB'"
  exit 0
}

$exe = Find-CnCodexExe -RepoRoot $repoRoot -Preferred $ExePath
Write-Step "Executable"
Write-Info $exe

Write-Step "Launching dual instances"
Start-IsolatedInstance -Exe $exe -ProjectRoot $nodeA -WebViewData $wvA -Title "Node A (Owner)"
Start-Sleep -Seconds 1
Start-IsolatedInstance -Exe $exe -ProjectRoot $nodeB -WebViewData $wvB -Title "Node B (Member)"

Write-Step "UI checklist"
Write-Host @"

1) Both windows: right sidebar -> lan collab panel
2) Display names:
     A = OwnerA
     B = MemberB
3) Enable LAN collab on both
4) Read A's local address/port (usually 47800)
5) On B, connect manually:
     127.0.0.1:<A_PORT>
6) A creates a group + invite code; B joins
7) Send chat messages both ways
8) A shares model/knowledge; B uses/fetch

Guide (Chinese):
  docs\superpowers\specs\2026-07-17-lan-p2p-single-pc-dual-client-test.md

Test data:
  $TestRoot

"@ -ForegroundColor Yellow

Write-Step "Done"
Write-Info "If the second window fails, check WEBVIEW2_USER_DATA_FOLDER is writable and unique."
