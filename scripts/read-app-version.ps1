$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$tauriConf = Join-Path $root "src-tauri\tauri.conf.json"
$cargoToml = Join-Path $root "src-tauri\Cargo.toml"

if (Test-Path -LiteralPath $tauriConf) {
    $json = Get-Content -LiteralPath $tauriConf -Raw | ConvertFrom-Json
    if ($json.version) {
        Write-Output ([string]$json.version).Trim()
        exit 0
    }
}

if (Test-Path -LiteralPath $cargoToml) {
    $match = Select-String -Path $cargoToml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if ($match) {
        Write-Output $match.Matches[0].Groups[1].Value.Trim()
        exit 0
    }
}

throw "Unable to resolve app version from tauri.conf.json / Cargo.toml"
