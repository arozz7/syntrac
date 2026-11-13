# Removes the SYNTRAC service registration. Run in an ELEVATED PowerShell. Compatible with Windows PowerShell 5.1.
#
# By default NOTHING is deleted except the service registration: your data, logs, configuration and the installed
# program files are kept. Deleting them needs an explicit switch, so a quick uninstall can never remove your history.
param(
    [switch]$RemoveProgramFiles,
    [switch]$RemoveData,
    [string]$InstallDir = (Join-Path $env:ProgramFiles 'Syntrac'),
    [string]$DataDir = (Join-Path $env:ProgramData 'Syntrac')
)

$ErrorActionPreference = 'Stop'
$ServiceName = 'Syntrac'

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'This script must run in an elevated (Administrator) PowerShell.'
}

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($service) {
    if ($service.Status -ne 'Stopped') {
        Write-Host 'Stopping the service...'
        Stop-Service -Name $ServiceName -Force
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(90))
    }
    & sc.exe delete $ServiceName | Out-Null
    Write-Host 'Service registration removed.'
} else {
    Write-Host 'The service is not registered.'
}

if ($RemoveProgramFiles -and (Test-Path $InstallDir)) {
    Remove-Item -Recurse -Force $InstallDir
    Write-Host "Removed $InstallDir"
}
if ($RemoveData -and (Test-Path $DataDir)) {
    Remove-Item -Recurse -Force $DataDir
    Write-Host "Removed $DataDir (database and logs)"
}

Write-Host ''
if (-not $RemoveProgramFiles) { Write-Host "Kept: $InstallDir (program files and config)" }
if (-not $RemoveData) { Write-Host "Kept: $DataDir (database and logs)" }