# Removes the SYNTRAC service registration. Run in an ELEVATED PowerShell. Compatible with Windows PowerShell 5.1.
#
# By default NOTHING is deleted except the service registration: your data, logs, configuration and the installed
# program files are kept. Deleting them needs an explicit switch, so a quick uninstall can never remove your history.
param(
    [switch]$RemoveProgramFiles,
    # Removes only the program folder (bin\), keeping the config and the data. What the installer's uninstaller asks for.
    [switch]$RemoveBinaries,
    [switch]$RemoveData,
    [string]$InstallDir = (Join-Path $env:ProgramFiles 'Syntrac'),
    # Which data folder -RemoveData would delete. Default: the one the installed config points at (the data may have been
    # moved to another drive), else C:\ProgramData\Syntrac.
    [string]$DataDir
)

$ErrorActionPreference = 'Stop'
$ServiceName = 'Syntrac'
. (Join-Path $PSScriptRoot 'config-helpers.ps1')
$installedConfig = Join-Path $InstallDir 'config\service.toml'
if (-not $DataDir -and (Test-Path $installedConfig)) { $DataDir = Get-DataRootFromConfig (Get-Content -Path $installedConfig -Raw) }
if (-not $DataDir) { $DataDir = Join-Path $env:ProgramData 'Syntrac' }

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

if ($RemoveBinaries -and -not $RemoveProgramFiles) {
    $binFolder = Join-Path $InstallDir 'bin'
    if (Test-Path $binFolder) {
        Remove-Item -Recurse -Force $binFolder
        Write-Host "Removed $binFolder (the program); the configuration is kept."
    }
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
if (-not $RemoveProgramFiles -and -not $RemoveBinaries) { Write-Host "Kept: $InstallDir (program files and config)" }
if ($RemoveBinaries -and -not $RemoveProgramFiles) { Write-Host "Kept: $InstallDir\config (configuration)" }
if (-not $RemoveData) { Write-Host "Kept: $DataDir (database and logs)" }