# Print "<drive>|<freeGB>" for the drive that contains -Path.
param(
    [Parameter(Mandatory = $true)]
    [string]$Path
)

$ErrorActionPreference = "Stop"

try {
    if (-not (Test-Path -LiteralPath $Path)) {
        Write-Output "?:|0"
        exit 0
    }

    $full = (Resolve-Path -LiteralPath $Path).Path
    $root = [System.IO.Path]::GetPathRoot($full)
    if ([string]::IsNullOrWhiteSpace($root)) {
        Write-Output "?:|0"
        exit 0
    }

    $deviceId = $root.TrimEnd('\', '/')
    if ($deviceId -notmatch ':$') {
        $deviceId = $deviceId + ':'
    }

    $drive = Get-CimInstance -ClassName Win32_LogicalDisk -Filter ("DeviceID='{0}'" -f $deviceId) -ErrorAction SilentlyContinue
    if ($null -eq $drive) {
        # Fallback for environments where CIM/WMI is slow or unavailable.
        $name = $deviceId.Substring(0, 1)
        $psDrive = Get-PSDrive -Name $name -ErrorAction SilentlyContinue
        if ($null -eq $psDrive) {
            Write-Output "?:|0"
            exit 0
        }
        Write-Output ("{0}|{1:N2}" -f $deviceId, ($psDrive.Free / 1GB))
        exit 0
    }

    Write-Output ("{0}|{1:N2}" -f $drive.DeviceID, ($drive.FreeSpace / 1GB))
    exit 0
}
catch {
    Write-Output "?:|0"
    exit 0
}
