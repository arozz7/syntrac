# Moves the installed collector's data to another drive. Run in an ELEVATED PowerShell. Compatible with Windows
# PowerShell 5.1.
#
#   scripts/deploy/move-data.ps1 -UserSid <your SID> -NewDataDir D:\Syntrac [-DryRun]
#
#   Get your SID (in a NORMAL PowerShell, not the elevated one):
#       [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
#
# What it does, in this order (and stops at the first problem):
#   1. checks the new location is a suitable drive (fixed internal disk, enough room), and that the CURRENT database
#      is healthy (SQLite integrity check)
#   2. stops the service
#   3. COPIES the database to <NewDataDir>\data (the old files are never touched or deleted)
#   4. proves the copy: same schema version, same row count in every table, integrity check passes
#   5. points the service's config at the new location (the old config is kept as service.toml.bak-<time>) and starts it
#   6. waits until the service reports the new path and is writing again
# If anything fails after the service was stopped, the config is restored and the service is started on the OLD
# database again. -DryRun does step 1 only and changes nothing (it works without elevation).
#
# The old database files are LEFT IN PLACE. Delete them yourself when you are satisfied; this script never deletes.
param(
    [Parameter(Mandatory = $true)][string]$UserSid,
    [Parameter(Mandatory = $true)][string]$NewDataDir,
    [string]$InstallDir = (Join-Path $env:ProgramFiles 'Syntrac'),
    # Which syntrac-service.exe does the database checking (it only READS files). Default: the installed one, which
    # must be a build that knows --check-db.
    [string]$CollectorExecutable,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$ServiceName = 'Syntrac'
. (Join-Path $PSScriptRoot 'config-helpers.ps1')
. (Join-Path $PSScriptRoot 'data-root.ps1')
. (Join-Path $PSScriptRoot 'service-common.ps1')

# A dry run only reads (config, drive facts, a read-only database check), so it needs no elevation; the real move does.
if (-not $DryRun) { Assert-Elevated }

# PowerShell variable names are case-insensitive, so locals must not share a name with a parameter.
$collectorExe = $CollectorExecutable
if (-not $collectorExe) { $collectorExe = Join-Path $InstallDir 'bin\syntrac-service.exe' }
$configFile = Join-Path $InstallDir 'config\service.toml'
foreach ($required in $collectorExe, $configFile) {
    if (-not (Test-Path -LiteralPath $required)) { throw "Not found: $required (is the service installed?)" }
}

$configText = Get-Content -Path $configFile -Raw
$currentDatabase = Get-ConfigValue $configText 'database_path'
if (-not $currentDatabase) { throw "The config $configFile has no database_path." }
$target = Get-ExpectedConfigPaths -DataRoot $NewDataDir
if ($currentDatabase -eq $target.Database) {
    Write-Host "The service already uses $($target.Database). Nothing to do."
    exit 0
}

$currentFile = $currentDatabase -replace '/', '\'
$targetFile = $target.Database -replace '/', '\'
$targetFolder = Split-Path -Parent $targetFile
if (-not (Test-Path -LiteralPath $currentFile)) { throw "The current database was not found: $currentFile" }
if (Test-Path -LiteralPath $targetFile) {
    throw "A database already exists at $targetFile. Refusing to overwrite it; choose another folder or move that one aside yourself."
}

# Bytes to be copied (database plus write-ahead log). Room for the copy AND some growth: twice its size, at least the floor.
$sourceBytes = 0
foreach ($suffix in '', '-wal') {
    $item = Get-Item -LiteralPath ($currentFile + $suffix) -ErrorAction SilentlyContinue
    if ($item) { $sourceBytes += $item.Length }
}
$needed = [math]::Max($script:DefaultMinFreeBytes, 2 * $sourceBytes)
Assert-DataRootAcceptable -DataRoot $NewDataDir -MinFreeBytes $needed

Write-Host 'Checking the current database (read-only)...'
$before = Invoke-Collector -Executable $collectorExe -Arguments @('--check-db', $currentFile)
if (($before.Lines -join ' ') -match 'unknown option .--check-db') {
    throw "$collectorExe is an older build that cannot check a database. Install the current build first (cargo build --release -p syntrac-service, then scripts/deploy/install-service.ps1 -UserSid <your SID>), or pass -CollectorExecutable <path to a newer syntrac-service.exe>."
}
if ($before.ExitCode -ne 0 -or -not (Test-DatabaseReportHealthy $before.Lines)) {
    throw ("The current database failed its health check, so it will not be copied:`n" + ($before.Lines -join "`n"))
}
$before.Lines | ForEach-Object { Write-Host "  $_" }

Write-Host ''
Write-Host "Plan: copy $([math]::Round($sourceBytes / 1MB, 1)) MB from $currentFile"
Write-Host "      to   $targetFile"
Write-Host "      then point the service at it. The old files stay where they are."
if ($DryRun) {
    Write-Host ''
    Write-Host 'Dry run: nothing was changed.'
    exit 0
}

$configBackup = $null
$configSwitched = $false
$serviceStopped = $false
try {
    $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if ($service -and $service.Status -ne 'Stopped') {
        Write-Host 'Stopping the service...'
        Stop-Service -Name $ServiceName -Force
        if (-not (Wait-ServiceState -Name $ServiceName -State 'Stopped' -Seconds 90)) { throw 'The service did not stop within 90 seconds.' }
        $serviceStopped = $true
    }

    Write-Host 'Preparing the new folder and its permissions...'
    Set-DataRootAcls -DataRoot $NewDataDir -UserSid $UserSid

    Write-Host 'Copying...'
    foreach ($suffix in '', '-wal') {
        $from = $currentFile + $suffix
        if (Test-Path -LiteralPath $from) { Copy-Item -LiteralPath $from -Destination ($targetFile + $suffix) }
    }
    # The shared-memory file (-shm) is rebuilt by SQLite and must not be copied.

    Write-Host 'Verifying the copy...'
    $source = Invoke-Collector -Executable $collectorExe -Arguments @('--check-db', $currentFile)
    $copy = Invoke-Collector -Executable $collectorExe -Arguments @('--check-db', $targetFile)
    if ($source.ExitCode -ne 0 -or $copy.ExitCode -ne 0) {
        throw ("A health check failed.`n  source: " + ($source.Lines -join ' | ') + "`n  copy:   " + ($copy.Lines -join ' | '))
    }
    $comparison = Compare-DatabaseReports -Source $source.Lines -Copy $copy.Lines
    if (-not $comparison.Identical) { throw ("The copy differs from the original:`n  " + ($comparison.Differences -join "`n  ")) }
    if (-not (Test-DatabaseReportHealthy $copy.Lines)) { throw 'The copy failed its integrity check.' }
    $copy.Lines | ForEach-Object { Write-Host "  $_" }

    Write-Host 'Switching the service to the new location...'
    $configBackup = "$configFile.bak-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    Copy-Item -LiteralPath $configFile -Destination $configBackup
    $newConfig = Set-ConfigPaths -ConfigText $configText -Database $target.Database -Logs $target.Logs
    [IO.File]::WriteAllText($configFile, $newConfig, (New-Object Text.UTF8Encoding $false))
    $configSwitched = $true

    Start-Service -Name $ServiceName
    if (-not (Wait-ServiceState -Name $ServiceName -State 'Running' -Seconds 60)) {
        throw "The service did not reach Running with the new location. See $(Join-Path $InstallDir 'bin\startup-error.txt') and the logs in $($target.Logs)."
    }

    Write-Host 'Waiting for the service to report the new database...'
    $reported = $false
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline -and -not $reported) {
        $status = Invoke-Collector -Executable $collectorExe -Arguments @('--status')
        if (($status.Lines -join "`n") -match [regex]::Escape($target.Database)) { $reported = $true } else { Start-Sleep -Seconds 2 }
    }
    if (-not $reported) { throw 'The service is running but does not report the new database path.' }
} catch {
    Write-Host ''
    Write-Host "FAILED: $_" -ForegroundColor Red
    if ($configSwitched -and $configBackup) {
        Write-Host 'Restoring the previous configuration...' -ForegroundColor Yellow
        Copy-Item -LiteralPath $configBackup -Destination $configFile -Force
    }
    if ($serviceStopped -or $configSwitched) {
        Write-Host 'Starting the service on the ORIGINAL database again...' -ForegroundColor Yellow
        try { Restart-Service -Name $ServiceName -Force } catch { Write-Host "Could not restart the service: $_" -ForegroundColor Red }
    }
    Write-Host 'Nothing was deleted. The original database is untouched.' -ForegroundColor Yellow
    exit 1
}

# Writing again?
Start-Sleep -Seconds 10
$after = Invoke-Collector -Executable $collectorExe -Arguments @('--check-db', $targetFile)
$eventsAfter = ($after.Lines | Where-Object { $_ -match '^table events ' } | ForEach-Object { [int64]($_ -split ' ')[2] } | Select-Object -First 1)
$eventsCopied = ($copy.Lines | Where-Object { $_ -match '^table events ' } | ForEach-Object { [int64]($_ -split ' ')[2] } | Select-Object -First 1)

Write-Host ''
Write-Host 'Moved.' -ForegroundColor Green
Write-Host "  New database:   $targetFile"
Write-Host "  New logs:       $($target.Logs)"
Write-Host "  Events: $eventsCopied copied, $eventsAfter now (the service is writing to the new database)"
Write-Host "  Old config kept as: $configBackup"
Write-Host ''
Write-Host 'The old database was NOT deleted. When you are satisfied, you can free the space yourself:'
Write-Host "  $currentFile (and its -wal / -shm files)"
Write-Host "  the old log folder next to it (history only; new logs go to $($target.Logs))"
