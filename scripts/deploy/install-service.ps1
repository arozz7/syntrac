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
. (Join-Path $PSScriptRoot 'data-root.ps1')
. (Join-Path $PSScriptRoot 'service-common.ps1')

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

# 0. Refuse an unsuitable data drive BEFORE changing anything (USB disk, network share, missing or nearly full drive).
#    An existing install already using this root is only re-checked for being a suitable kind of drive, not for free
#    space, so an upgrade is never blocked on a drive that is simply getting full.
$alreadyUsesThisRoot = (Test-Path $configFile) -and
    ((Get-ConfigValue (Get-Content -Path $configFile -Raw) 'database_path') -eq $expectedPaths.Database)
$minFree = $script:DefaultMinFreeBytes
if ($alreadyUsesThisRoot) { $minFree = 0 }
Assert-DataRootAcceptable -DataRoot $dataRoot -MinFreeBytes $minFree

# 1. Stop a running service before replacing its binary.
$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing -and $existing.Status -ne 'Stopped') {
    Write-Host 'Stopping the running service for the upgrade...'
    Stop-Service -Name $ServiceName -Force
    if (-not (Wait-ServiceState -Name $ServiceName -State 'Stopped' -Seconds 90)) { throw 'The service did not stop within 90 seconds.' }
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
#    and only read logs\. Nobody else gets in. (The rules live in data-root.ps1, shared with move-data.ps1.)
Set-DataRootAcls -DataRoot $dataRoot -UserSid $UserSid

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
# Restart after failures: after 5 s, then 30 s, then every 60 s for as long as it keeps failing (the last action
# repeats), so a data drive that is not yet available at boot is retried until it appears. The failure count resets
# daily. Failures include non-zero stops.
& sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/30000/restart/60000 | Out-Null
& sc.exe failureflag $ServiceName 1 | Out-Null

# 5. Start it.
if (-not $NoStart) {
    Start-Service -Name $ServiceName
    if (-not (Wait-ServiceState -Name $ServiceName -State 'Running' -Seconds 60)) {
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