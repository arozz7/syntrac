# Run by the installer after it has copied its files: installs or upgrades the collector service from the payload next to this script.
# Compatible with Windows PowerShell 5.1. Exits 0 on success and 1 on failure, with everything it did in the log.
#
# An upgrade is made safe like this, and a failure puts back what it saved:
#   1. The new program is asked which database schema it uses. A newer schema migrates the data when the service starts, and an older program refuses
#      a newer database, so the data is copied first (and the upgrade does not start without room for the copy).
#   2. The service is stopped; the old program, the config and (when needed) the database are copied aside.
#   3. install-service.ps1 does the real work. If it fails, the saved copies are restored and the old service is started again.
# The installer's own files (the app) are replaced by the installer before this runs and are NOT rolled back.
param(
    [Parameter(Mandatory = $true)][string]$InstallDir,
    # The user who runs the app. Left out, it is the installed config's user (upgrade) or the signed-in desktop user (first install).
    [string]$UserSid,
    [string]$LogPath = (Join-Path $env:TEMP 'SYNTRAC-setup.log')
)

$ErrorActionPreference = 'Stop'
$setupRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
. (Join-Path $PSScriptRoot 'config-helpers.ps1')
. (Join-Path $PSScriptRoot 'data-root.ps1')
. (Join-Path $PSScriptRoot 'service-common.ps1')
. (Join-Path $PSScriptRoot 'installer-helpers.ps1')

$ServiceName = 'Syntrac'

# The signed-in desktop user: whoever owns the shell in this session. The installer itself may run as another administrator account.
function Get-DesktopUserSid {
    $session = [Diagnostics.Process]::GetCurrentProcess().SessionId
    $shell = Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session } | Select-Object -First 1
    if (-not $shell) { return $null }
    $owner = Invoke-CimMethod -InputObject $shell -MethodName GetOwnerSid -ErrorAction SilentlyContinue
    if ($owner -and $owner.ReturnValue -eq 0) { return $owner.Sid }
    return $null
}

function Copy-DatabaseFiles {
    param([string]$From, [string]$To)
    foreach ($suffix in '', '-wal', '-shm') {
        if (Test-Path "$From$suffix") { Copy-Item -Path "$From$suffix" -Destination "$To$suffix" -Force }
    }
}

function Get-FreeBytes {
    param([string]$Path)
    $root = [IO.Path]::GetPathRoot($Path)
    return (New-Object IO.DriveInfo($root)).AvailableFreeSpace
}

function Write-Step { param([string]$Text) Write-Host ("[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $Text) }

# What a rollback needs to know. `Changed` turns true just before the first thing is altered: a failure before that (a refused folder, no room for
# the copy) changes nothing, so there is nothing to put back.
$saved = @{ HadProgram = $false; Config = $false; Database = $false; WasRunning = $false; Changed = $false }

try {
    Start-Transcript -Path $LogPath -Append | Out-Null
    Write-Step "SYNTRAC setup: installing into $InstallDir"
    Assert-Elevated

    $programFiles = $env:ProgramW6432
    if (-not $programFiles) { $programFiles = $env:ProgramFiles }
    if (-not (Test-InstallDirAcceptable -InstallDir $InstallDir -ProgramFiles $programFiles)) {
        throw "SYNTRAC must be installed inside '$programFiles': its service runs as SYSTEM, so the folder must not be writable by ordinary users. Chosen: '$InstallDir'."
    }

    $newExe = Join-Path $setupRoot 'syntrac-service.exe'
    if (-not (Test-Path $newExe)) { throw "The installer's service program is missing: $newExe" }
    $binDir = Join-Path $InstallDir 'bin'
    $installedExe = Join-Path $binDir 'syntrac-service.exe'
    $configFile = Join-Path $InstallDir 'config\service.toml'
    $previousExe = "$installedExe.previous"
    $configCopy = "$configFile.before-upgrade"

    $hadProgram = Test-Path $installedExe
    $saved.HadProgram = $hadProgram
    $installedConfig = ''
    if (Test-Path $configFile) { $installedConfig = Get-Content -Path $configFile -Raw }

    # Who reads the data, and where the data is.
    $reader = Resolve-ReadingUserSid -Override $UserSid -InstalledConfigText $installedConfig -ConsoleUserSid (Get-DesktopUserSid)
    Write-Step "The app is run by $($reader.Sid) (from the $($reader.Source))"
    $dataRoot = Get-DataRootFromConfig $installedConfig
    if (-not $dataRoot) { $dataRoot = Join-Path $env:ProgramData 'Syntrac' }
    $databaseFile = Join-Path $dataRoot 'data\syntrac.db'
    $databaseCopy = "$databaseFile.before-upgrade"

    # Before anything changes: may this upgrade go on?
    $shipped = Read-SchemaVersion (Invoke-Collector -Executable $newExe -Arguments @('--version')).Lines
    $inDatabase = $null
    $databaseBytes = 0
    if (Test-Path $databaseFile) {
        # The new program reads it (any older schema is readable, and it is quick: --check-db reads the whole file, minutes on a large one).
        # A program that does not know --db-schema, or cannot start, prints no schema; the installed program's full check is the fallback.
        $inDatabase = Read-SchemaVersion (Invoke-Collector -Executable $newExe -Arguments @('--db-schema', $databaseFile)).Lines
        if ($null -eq $inDatabase -and $hadProgram) {
            Write-Step 'The new program did not report the database schema; asking the installed program (slower)'
            $inDatabase = Read-SchemaVersion (Invoke-Collector -Executable $installedExe -Arguments @('--check-db', $databaseFile)).Lines
        }
        $databaseBytes = (Get-Item $databaseFile).Length
        foreach ($suffix in '-wal') { if (Test-Path "$databaseFile$suffix") { $databaseBytes += (Get-Item "$databaseFile$suffix").Length } }
    }
    $plan = Get-UpgradePlan -ShippedSchema $shipped -DatabaseSchema $inDatabase -DatabaseExists (Test-Path $databaseFile) -DatabaseBytes $databaseBytes -FreeBytes (Get-FreeBytes $dataRoot)
    Write-Step "New program: schema v$shipped. Database: schema v$inDatabase. Plan: $($plan.Reason)"
    if (-not $plan.CanProceed) { throw $plan.Reason }

    # From here on things change.
    $saved.Changed = $true
    # Stop the running service, then save what a rollback needs.
    $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    $saved.WasRunning = [bool]($service -and $service.Status -ne 'Stopped')
    if ($saved.WasRunning) {
        Write-Step 'Stopping the running service'
        Stop-Service -Name $ServiceName -Force
        if (-not (Wait-ServiceState -Name $ServiceName -State 'Stopped' -Seconds 90)) { throw 'The service did not stop within 90 seconds.' }
    }
    if ($hadProgram) {
        Copy-Item -Path $installedExe -Destination $previousExe -Force
        if (Test-Path $configFile) { Copy-Item -Path $configFile -Destination $configCopy -Force; $saved.Config = $true }
        Write-Step "Kept the previous program as $previousExe"
    }
    if ($plan.NeedsDatabaseCopy) {
        Write-Step "Copying the database to $databaseCopy before it is migrated"
        Copy-DatabaseFiles -From $databaseFile -To $databaseCopy
        $saved.Database = $true
    }

    # The real work.
    Write-Step 'Installing the service'
    & (Join-Path $PSScriptRoot 'install-service.ps1') -UserSid $reader.Sid -Executable $newExe -InstallDir $InstallDir
    Write-Step 'The service is running'

    # The safety copy of the database is ours: it is removed once the new version has opened the data and it checks out.
    if ($saved.Database) {
        $deadline = (Get-Date).AddSeconds(30)
        $report = $null
        do {
            $report = Invoke-Collector -Executable $installedExe -Arguments @('--check-db', $databaseFile)
            if ($report.ExitCode -eq 0 -and (Read-SchemaVersion $report.Lines) -eq $shipped) { break }
            Start-Sleep -Seconds 2
        } while ((Get-Date) -lt $deadline)
        if ($report.ExitCode -eq 0 -and (Read-SchemaVersion $report.Lines) -eq $shipped) {
            foreach ($suffix in '', '-wal', '-shm') { if (Test-Path "$databaseCopy$suffix") { Remove-Item -Path "$databaseCopy$suffix" -Force } }
            Write-Step 'The migrated database checks out; the safety copy was removed'
        } else {
            Write-Step "WARNING: could not confirm the migrated database; the safety copy was KEPT at $databaseCopy"
        }
    }
    Write-Step 'SYNTRAC setup finished'
    exit 0
} catch {
    Write-Host "ERROR: $($_.Exception.Message)"
    try {
        if (-not $saved.Changed) { Write-Host 'Nothing had been changed, so there is nothing to put back.'; Write-Host "Log: $LogPath"; exit 1 }
        Write-Host 'Putting back what was there before...'
        $steps = Get-RollbackSteps -HadPreviousProgram $saved.HadProgram -HadConfigCopy $saved.Config -HadDatabaseCopy $saved.Database -WasRunning $saved.WasRunning
        foreach ($step in $steps) {
            Write-Step "rollback: $step"
            switch ($step) {
                'stop-service' {
                    $running = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
                    if ($running -and $running.Status -ne 'Stopped') { Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue; Wait-ServiceState -Name $ServiceName -State 'Stopped' -Seconds 60 | Out-Null }
                }
                'restore-program' {
                    # No copy means the program was never touched (the failure came before install-service replaced it).
                    if (Test-Path $previousExe) { Copy-Item -Path $previousExe -Destination $installedExe -Force } else { Write-Host 'The program was not replaced yet.' }
                }
                'restore-config' { Copy-Item -Path $configCopy -Destination $configFile -Force }
                'restore-database' {
                    # The files of the migrated database (including its log files) go; the copy made before takes their place.
                    foreach ($suffix in '-wal', '-shm') { if (Test-Path "$databaseFile$suffix") { Remove-Item -Path "$databaseFile$suffix" -Force } }
                    Copy-DatabaseFiles -From $databaseCopy -To $databaseFile
                }
                'start-service' { Start-Service -Name $ServiceName }
                'remove-service' { & sc.exe delete $ServiceName | Out-Null }
            }
        }
        Write-Host 'Rollback finished.'
    } catch {
        Write-Host "ROLLBACK FAILED: $($_.Exception.Message)"
    }
    Write-Host "Log: $LogPath"
    exit 1
} finally {
    try { Stop-Transcript | Out-Null } catch { }
}
