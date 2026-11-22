# The decisions of the installer (installer-install.ps1, installer-uninstall.ps1), as pure functions so they can be tested without installing
# anything. Needs config-helpers.ps1 (Get-ConfigValue). Compatible with Windows PowerShell 5.1.

$script:SidPattern = '^S-1-\d+(-\d+)+$'
# The SYSTEM account runs the service; it is never the person who reads the data.
$script:SystemSid = 'S-1-5-18'

function Test-ReadingUserSid {
    param([string]$Sid)
    return [bool]($Sid -and $Sid -match $script:SidPattern -and $Sid -ne $script:SystemSid)
}

# Who the service's pipe and data folder are opened to: the person who runs the app. In order:
#   1. an explicit -Override (a silent install says so on its command line);
#   2. on an upgrade, the user already recorded in the installed config (another administrator may be running the upgrade, and the config
#      must not be regenerated for the wrong reader);
#   3. the user signed in on the desktop (the elevated installer may run as a different administrator account).
# When none of them gives a valid user it refuses: guessing would open the data to the wrong account.
function Resolve-ReadingUserSid {
    param([string]$Override, [string]$InstalledConfigText, [string]$ConsoleUserSid)
    if ($Override) {
        if (-not (Test-ReadingUserSid $Override)) { throw "The user '$Override' is not a valid account SID." }
        return [pscustomobject]@{ Sid = $Override; Source = 'override' }
    }
    $installed = Get-ConfigValue $InstalledConfigText 'pipe_allowed_sid'
    if (Test-ReadingUserSid $installed) { return [pscustomobject]@{ Sid = $installed; Source = 'installed config' } }
    if (Test-ReadingUserSid $ConsoleUserSid) { return [pscustomobject]@{ Sid = $ConsoleUserSid; Source = 'desktop user' } }
    throw 'Could not tell which user runs SYNTRAC. Run the installer from the signed-in account, or pass /USERSID=<the account SID>.'
}

# The service runs as SYSTEM, so a folder ordinary users can write to would let them run code as SYSTEM. Only a folder inside Program Files is accepted.
function Test-InstallDirAcceptable {
    param([Parameter(Mandatory = $true)][string]$InstallDir, [Parameter(Mandatory = $true)][string]$ProgramFiles)
    if ($InstallDir -match '(^|[\\/])\.\.([\\/]|$)') { return $false }
    $folder = $InstallDir.TrimEnd('\', '/')
    $base = $ProgramFiles.TrimEnd('\', '/')
    return $folder.StartsWith($base + '\', [StringComparison]::OrdinalIgnoreCase)
}

# The number on the "schema:    vN" line of what the service prints for --version or --check-db, or $null.
function Read-SchemaVersion {
    param([string[]]$Lines)
    foreach ($line in $Lines) {
        if ($line -match '^\s*schema:\s+v(\d+)\s*$') { return [int]$Matches[1] }
    }
    return $null
}

# Whether an upgrade may start, and whether it first copies the database. A new version that raises the schema migrates the database when it
# starts, and an older program refuses a newer database: the copy is what lets a rollback put the data back. Nothing is changed unless there is
# room for the copy.
function Get-UpgradePlan {
    param($ShippedSchema, $DatabaseSchema, [long]$DatabaseBytes, [long]$FreeBytes)
    $refuse = { param($why) [pscustomobject]@{ CanProceed = $false; NeedsDatabaseCopy = $false; Reason = $why } }
    if ($null -eq $ShippedSchema) { return & $refuse 'The new program did not report which database schema it uses.' }
    if ($null -eq $DatabaseSchema) { return [pscustomobject]@{ CanProceed = $true; NeedsDatabaseCopy = $false; Reason = 'no database yet' } }
    if ($DatabaseSchema -gt $ShippedSchema) {
        return & $refuse "The database (schema v$DatabaseSchema) is newer than this version understands (schema v$ShippedSchema). Install a newer version."
    }
    if ($DatabaseSchema -eq $ShippedSchema) { return [pscustomobject]@{ CanProceed = $true; NeedsDatabaseCopy = $false; Reason = 'same schema' } }
    $needed = $DatabaseBytes + [long]($DatabaseBytes * 0.1) + 100MB
    if ($FreeBytes -lt $needed) {
        return & $refuse "There is not enough free space for a safety copy of the database before the upgrade ($([math]::Round($needed / 1GB, 1)) GB needed on that drive). Free some space, or lower the retention, and run the installer again."
    }
    return [pscustomobject]@{ CanProceed = $true; NeedsDatabaseCopy = $true; Reason = 'the schema changes' }
}

# What a failed install puts back, in order. Only what was saved is restored. The old service is started again only if it was running.
function Get-RollbackSteps {
    param([bool]$HadPreviousProgram, [bool]$HadConfigCopy, [bool]$HadDatabaseCopy, [bool]$WasRunning)
    $steps = @('stop-service')
    if (-not $HadPreviousProgram) { return $steps + 'remove-service' }
    $steps += 'restore-program'
    if ($HadConfigCopy) { $steps += 'restore-config' }
    if ($HadDatabaseCopy) { $steps += 'restore-database' }
    if ($WasRunning) { $steps += 'start-service' }
    return $steps
}
