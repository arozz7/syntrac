# The elevated half of smoke-test-service.ps1. Do not run directly; the unelevated wrapper starts it (one UAC
# prompt) and talks to it through marker files in -RunDir. Compatible with Windows PowerShell 5.1.
param(
    [Parameter(Mandatory = $true)][string]$UserSid,
    [Parameter(Mandatory = $true)][string]$RunDir,
    [Parameter(Mandatory = $true)][string]$Executable,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$ServiceName = 'Syntrac'
$log = Join-Path $RunDir 'elevated.log'

function Write-Log {
    param([string]$Message)
    $line = "$(Get-Date -Format 'HH:mm:ss') $Message"
    Add-Content -Path $log -Value $line
    Write-Host $line
}

function Set-Marker {
    param([string]$Name, [string]$Content = 'ok')
    Set-Content -Path (Join-Path $RunDir $Name) -Value $Content -Encoding ascii
}

function Wait-Marker {
    param([string]$Name, [int]$Seconds)
    $path = Join-Path $RunDir $Name
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $path) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

$failed = $false
try {
    Write-Log 'Installing and starting the service'
    & (Join-Path $PSScriptRoot 'install-service.ps1') -UserSid $UserSid -Executable $Executable
    Set-Marker 'phase1.ready'

    if (-not (Wait-Marker 'phase2.go' 600)) { throw 'Timed out waiting for the unelevated checks.' }

    Write-Log 'Stopping the service (timing it)'
    $service = Get-Service -Name $ServiceName
    $timer = [Diagnostics.Stopwatch]::StartNew()
    Stop-Service -Name $ServiceName -Force
    $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(90))
    $timer.Stop()
    $left = @(Get-Process -Name 'syntrac-service' -ErrorAction SilentlyContinue).Count
    Set-Marker 'phase2.done' ("stopSeconds=" + [math]::Round($timer.Elapsed.TotalSeconds, 1) + ";processesLeft=" + $left)
    Write-Log "Stopped in $([math]::Round($timer.Elapsed.TotalSeconds, 1)) s; processes left: $left"

    Write-Log 'Starting the service again (restart check)'
    Start-Service -Name $ServiceName
    $service = Get-Service -Name $ServiceName
    $service.WaitForStatus('Running', [TimeSpan]::FromSeconds(60))
    Set-Marker 'phase3.done' 'running'

    if ($Uninstall) {
        if (-not (Wait-Marker 'phase4.go' 300)) { throw 'Timed out waiting before uninstall.' }
        Write-Log 'Uninstalling the service registration (data is kept)'
        & (Join-Path $PSScriptRoot 'uninstall-service.ps1')
        Set-Marker 'phase4.done'
    }
} catch {
    $failed = $true
    Write-Log "ERROR: $_"
    Set-Marker 'error.txt' "$_"
} finally {
    Set-Marker 'elevated.done' 'done'
}

if ($failed) { Read-Host 'An error occurred (see above). Press Enter to close' } else { Start-Sleep -Seconds 4 }