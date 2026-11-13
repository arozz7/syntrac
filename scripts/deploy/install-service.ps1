# Installs (or upgrades) the SYNTRAC collector as a Windows service. Run in an ELEVATED PowerShell.
# Compatible with Windows PowerShell 5.1.
#
# Layout and why:
#   <InstallDir>\bin\syntrac-service.exe     administrators only: the service runs as SYSTEM, so a user-writable
#   <InstallDir>\config\service.toml         exe or config would be a privilege-escalation path
#   <DataDir>\data\syntrac.db                the reading user (-UserSid) may MODIFY it: SQLite needs to create the
#                                            WAL "-shm" file so the unelevated UI can read while the service writes
#   <DataDir>\logs\                          a SIBLING of data\, not inside it: the user may only READ it
#
# Re-running upgrades the binary in place. An existing config is kept unless its database/log paths are wrong, in
# which case it is renamed to a timestamped backup (never deleted) and regenerated.
param(
    [Parameter(Mandatory = $true)][string]$UserSid,
    [string]$Executable,
    [string]$InstallDir = (Join-Path $env:ProgramFiles 'Syntrac'),
    [string]$DataDir = (Join-Path $env:ProgramData 'Syntrac'),
    [switch]$NoStart
)

$ErrorActionPreference = 'Stop'
$ServiceName = 'Syntrac'
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '../..')
. (Join-Path $PSScriptRoot 'config-helpers.ps1')

function Assert-Elevated {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'This script must run in an elevated (Administrator) PowerShell.'
    }
}

function Invoke-Icacls {
    param([string[]]$Arguments)
    & icacls.exe @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "icacls failed ($LASTEXITCODE): $($Arguments -join ' ')" }
}

function Wait-ServiceState {
    param([string]$State, [int]$Seconds)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
        if ($service -and $service.Status.ToString() -eq $State) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

Assert-Elevated

# The SID identifies the person who will run the UI. It must be captured BEFORE elevation: an elevated
# session can belong to a different administrator account.
try {
    $account = (New-Object Security.Principal.SecurityIdentifier($UserSid)).Translate([Security.Principal.NTAccount]).Value
} catch {
    throw "UserSid '$UserSid' is not a valid SID for this machine."
}
Write-Host "Reading user: $account ($UserSid)"

if (-not $Executable) { $Executable = Join-Path $repoRoot 'target/release/syntrac-service.exe' }
if (-not (Test-Path $Executable)) { throw "Executable not found: $Executable (run: cargo build --release -p syntrac-service)" }

# PowerShell variable names are case-insensitive: a local called $dataDir WOULD BE the -DataDir parameter. The data
# root and its sub-folders therefore get clearly different names (this exact collision once put the logs inside the
# user-writable data folder).
$binDir = Join-Path $InstallDir 'bin'
$configDir = Join-Path $InstallDir 'config'
$dataRoot = $DataDir
$databaseFolder = Join-Path $dataRoot 'data'
$logFolder = Join-Path $dataRoot 'logs'
$expectedPaths = Get-ExpectedConfigPaths -DataRoot $dataRoot
$installedExe = Join-Path $binDir 'syntrac-service.exe'
$configFile = Join-Path $configDir 'service.toml'

# 1. Stop a running service before replacing its binary.
$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing -and $existing.Status -ne 'Stopped') {
    Write-Host 'Stopping the running service for the upgrade...'
    Stop-Service -Name $ServiceName -Force
    if (-not (Wait-ServiceState -State 'Stopped' -Seconds 90)) { throw 'The service did not stop within 90 seconds.' }
}

# 2. Program files (inherit Program Files permissions: administrators write, users only read/execute).
New-Item -ItemType Directory -Force -Path $binDir, $configDir | Out-Null
Copy-Item -Path $Executable -Destination $installedExe -Force

$writeConfig = $true
if (Test-Path $configFile) {
    $existingText = Get-Content -Path $configFile -Raw
    if (Test-ConfigPaths $existingText $expectedPaths.Database $expectedPaths.Logs $UserSid) {
        $writeConfig = $false
        Write-Host "Kept the existing $configFile"
    } else {
        $backup = "$configFile.bak-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
        Move-Item -Path $configFile -Destination $backup
        Write-Host "The existing config had wrong database/log/pipe settings; renamed it to $backup and regenerating"
    }
}
if ($writeConfig) {
    $template = Get-Content -Path (Join-Path $repoRoot 'config/service.toml') -Raw
    $text = New-ConfigText -Template $template -Database $expectedPaths.Database -Logs $expectedPaths.Logs -UserSid $UserSid
    [IO.File]::WriteAllText($configFile, $text, (New-Object Text.UTF8Encoding $false))
    Write-Host "Wrote $configFile"
}

# 3. Data folder: SYSTEM and Administrators full control; the reading user may traverse the root, modify data\,
#    and only read logs\. Nobody else gets in.
New-Item -ItemType Directory -Force -Path $dataRoot | Out-Null
Invoke-Icacls @($dataRoot, '/inheritance:r', '/grant:r', '*S-1-5-18:(OI)(CI)F', '*S-1-5-32-544:(OI)(CI)F', "*${UserSid}:(RX)")
New-Item -ItemType Directory -Force -Path $databaseFolder, $logFolder | Out-Null
Invoke-Icacls @($databaseFolder, '/grant:r', "*${UserSid}:(OI)(CI)M")
Invoke-Icacls @($logFolder, '/grant:r', "*${UserSid}:(OI)(CI)RX")

# 4. Register the service (LocalSystem; ETW needs high privileges). If the registered path differs, recreate it.
$binaryPath = "`"$installedExe`" --service"
$registered = Get-CimInstance Win32_Service -Filter "Name='$ServiceName'" -ErrorAction SilentlyContinue
if ($registered -and $registered.PathName -ne $binaryPath) {
    Write-Host 'The registered path differs; recreating the service registration.'
    & sc.exe delete $ServiceName | Out-Null
    Start-Sleep -Seconds 2
    $registered = $null
}
if (-not $registered) {
    New-Service -Name $ServiceName -BinaryPathName $binaryPath -DisplayName 'SYNTRAC Collector' -StartupType Automatic | Out-Null
}
Set-Service -Name $ServiceName -Description 'SYNTRAC workstation activity collector. Writes local telemetry to SQLite; sends nothing off the machine.'
& sc.exe config $ServiceName start= delayed-auto | Out-Null
# Restart after failures: 5 s, then 30 s; reset the failure count daily. Failures include non-zero stops.
& sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/30000// | Out-Null
& sc.exe failureflag $ServiceName 1 | Out-Null

# 5. Start it.
if (-not $NoStart) {
    Start-Service -Name $ServiceName
    if (-not (Wait-ServiceState -State 'Running' -Seconds 60)) {
        $reason = Join-Path $binDir 'startup-error.txt'
        $detail = ''
        if (Test-Path $reason) { $detail = ' Reason: ' + (Get-Content $reason -Raw) }
        throw "The service did not reach Running within 60 seconds.$detail Logs: $logFolder"
    }
    Write-Host 'Service is running.'
}

Write-Host ''
Write-Host "Installed.  Program: $InstallDir   Data: $dataRoot"
Write-Host "Database:   $(Join-Path $databaseFolder 'syntrac.db')"
Write-Host "Logs:       $logFolder"