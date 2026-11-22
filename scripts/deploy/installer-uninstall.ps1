# Run by the uninstaller before it removes its files: stops and removes the collector service and its program folder (bin\).
# Compatible with Windows PowerShell 5.1. Exits 0 on success and 1 on failure.
#
# The configuration (<InstallDir>\config) and the data (the database and logs) are KEPT: removing them is a decision for the person, taken by hand
# with uninstall-service.ps1 -RemoveProgramFiles / -RemoveData. The installer calls this only for a real uninstall, never while an upgrade replaces
# the program.
param(
    [Parameter(Mandatory = $true)][string]$InstallDir,
    [string]$LogPath = (Join-Path $env:TEMP 'SYNTRAC-uninstall.log')
)

$ErrorActionPreference = 'Stop'
try {
    Start-Transcript -Path $LogPath -Append | Out-Null
    Write-Host "SYNTRAC uninstall: removing the service of $InstallDir"
    & (Join-Path $PSScriptRoot 'uninstall-service.ps1') -InstallDir $InstallDir -RemoveBinaries
    Write-Host 'Done.'
    exit 0
} catch {
    Write-Host "ERROR: $($_.Exception.Message)"
    Write-Host "Log: $LogPath"
    exit 1
} finally {
    try { Stop-Transcript | Out-Null } catch { }
}
