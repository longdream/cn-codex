param([Parameter(Mandatory = $true)][string]$Workspace)
$ErrorActionPreference = "Stop"
Push-Location $Workspace
try {
  $output = & node --test 2>&1 | Out-String
  $code = $LASTEXITCODE
} finally {
  Pop-Location
}
$pass = if ($code -eq 0) { 1 } else { 0 }
$reason = if ($pass) { "tests passed" } else { "tests failed: $output" }
return @{ pass = $pass; reason = $reason }