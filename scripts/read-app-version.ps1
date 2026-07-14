$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $root "src-tauri\Cargo.toml"

if (Test-Path -LiteralPath $cargoToml) {
    $match = Select-String -Path $cargoToml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if ($match) {
        Write-Output $match.Matches[0].Groups[1].Value.Trim()
        exit 0
    }
}

throw "Unable to resolve app version from src-tauri/Cargo.toml (single source of truth)"
