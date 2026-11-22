# Rules and helpers for WHERE the collector keeps its data (the "data root", default C:\ProgramData\Syntrac).
# Shared by install-service.ps1 and move-data.ps1. Compatible with Windows PowerShell 5.1.
#
# The decision (Test-DataRootChoice) is pure: it is handed facts about the volume, so each case is tested without
# the hardware (see data-root.tests.ps1). Reading those facts (Get-VolumeFacts) and applying permissions
# (Set-DataRootAcls) touch the machine and are verified by the smoke test.

# A data drive needs room for the database plus its write-ahead log and some growth. This is a floor for refusing a
# clearly unsuitable drive, not a size forecast.
$script:DefaultMinFreeBytes = 20GB

# Buses that can disappear under a running service, or that are not a disk in this machine.
$script:RemovableBuses = @('USB', '1394', 'iSCSI')

function Test-DataRootChoice {
    param(
        [Parameter(Mandatory = $true)][string]$DataRoot,
        [Parameter(Mandatory = $true)]$Volume,
        [int64]$MinFreeBytes = $script:DefaultMinFreeBytes
    )
    $problems = New-Object System.Collections.ArrayList

    if ($DataRoot.StartsWith('\\')) {
        [void]$problems.Add("$DataRoot is a network share. The collector's data must be on a disk inside this machine.")
    } elseif ($DataRoot -notmatch '^[A-Za-z]:[\\/]') {
        [void]$problems.Add("$DataRoot is not an absolute path on a drive letter (the service starts in System32, so a relative path would land in the wrong place).")
    } else {
        $drive = $DataRoot.Substring(0, 2).ToUpper()
        if (-not $Volume.Exists) {
            [void]$problems.Add("Drive $drive is not available on this machine.")
        } else {
            if ($Volume.DriveType -ne 'Fixed') {
                [void]$problems.Add("Drive $drive is a $($Volume.DriveType) drive. The data must be on a fixed disk.")
            }
            if ($script:RemovableBuses -contains $Volume.BusType) {
                [void]$problems.Add("Drive $drive is on a $($Volume.BusType)-connected disk. It can be unplugged or powered off while the service runs, which would stop collection.")
            }
            if ($Volume.FreeBytes -lt $MinFreeBytes) {
                [void]$problems.Add(("Drive $drive has only {0:N1} GB free; at least {1:N1} GB is required." -f ($Volume.FreeBytes / 1GB), ($MinFreeBytes / 1GB)))
            }
        }
    }
    return [pscustomobject]@{ Ok = ($problems.Count -eq 0); Problems = [string[]]$problems.ToArray() }
}

# The facts of a drive from the plain .NET drive information (ready, type, free space). The disk bus is not known this way.
function ConvertTo-VolumeFactsFromDriveInfo {
    param([bool]$IsReady, [string]$DriveType, [int64]$FreeBytes)
    return [pscustomobject]@{ Exists = $IsReady; DriveType = $DriveType; BusType = 'Unknown'; FreeBytes = $FreeBytes }
}

# Facts about the volume a data root would live on. A UNC path or an unreadable volume yields "does not exist"
# facts; Test-DataRootChoice has already refused a UNC path for its own reason.
function Get-VolumeFacts {
    param([Parameter(Mandatory = $true)][string]$DataRoot)
    $facts = [pscustomobject]@{ Exists = $false; DriveType = 'Unknown'; BusType = 'Unknown'; FreeBytes = [int64]0 }
    if ($DataRoot -notmatch '^[A-Za-z]:') { return $facts }
    $letter = $DataRoot.Substring(0, 1)
    $volume = Get-Volume -DriveLetter $letter -ErrorAction SilentlyContinue
    if (-not $volume) {
        # The storage cmdlets can know nothing about a drive that is fine (Windows Sandbox, some virtual machines): ask .NET instead.
        $drive = New-Object IO.DriveInfo($letter)
        if ($drive.IsReady) { return ConvertTo-VolumeFactsFromDriveInfo -IsReady $true -DriveType $drive.DriveType.ToString() -FreeBytes $drive.AvailableFreeSpace }
        return $facts
    }
    $facts.Exists = $true
    $facts.DriveType = [string]$volume.DriveType
    $facts.FreeBytes = [int64]$volume.SizeRemaining
    try {
        $facts.BusType = [string](Get-Partition -DriveLetter $letter -ErrorAction Stop | Get-Disk -ErrorAction Stop).BusType
    } catch {
        $facts.BusType = 'Unknown'
    }
    return $facts
}

# Stops with a readable message listing every problem.
function Assert-DataRootAcceptable {
    param([Parameter(Mandatory = $true)][string]$DataRoot, [int64]$MinFreeBytes = $script:DefaultMinFreeBytes)
    $verdict = Test-DataRootChoice -DataRoot $DataRoot -Volume (Get-VolumeFacts -DataRoot $DataRoot) -MinFreeBytes $MinFreeBytes
    if (-not $verdict.Ok) {
        throw ("The data folder '$DataRoot' cannot be used:`n  - " + ($verdict.Problems -join "`n  - "))
    }
}

# --- proving that a copy of the database is a faithful copy. The reports are the lines printed by
#     `syntrac-service --check-db <file>`: the first line is the path, the rest are facts that must match exactly.
function Test-DatabaseReportHealthy {
    param([string[]]$Report)
    return (@($Report) -contains 'integrity: ok')
}

function Compare-DatabaseReports {
    param([string[]]$Source, [string[]]$Copy)
    $sourceFacts = @($Source | Select-Object -Skip 1)
    $copyFacts = @($Copy | Select-Object -Skip 1)
    $differences = New-Object System.Collections.ArrayList
    foreach ($line in $sourceFacts) { if ($copyFacts -notcontains $line) { [void]$differences.Add("source has '$line', the copy does not") } }
    foreach ($line in $copyFacts) { if ($sourceFacts -notcontains $line) { [void]$differences.Add("the copy has '$line', the source does not") } }
    return [pscustomobject]@{ Identical = ($differences.Count -eq 0); Differences = [string[]]$differences.ToArray() }
}

function Invoke-Icacls {
    param([string[]]$Arguments)
    & icacls.exe @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "icacls failed ($LASTEXITCODE): $($Arguments -join ' ')" }
}

# Creates the data root with its two sub-folders and sets who may do what:
#   the root          SYSTEM and Administrators full control; the reading user may only traverse it
#   data\             the reading user may MODIFY (SQLite must create the WAL "-shm" file so an unelevated reader works)
#   logs\             a SIBLING of data\; the reading user may only READ it
# Nobody else gets in. Safe to run again on an existing root.
function Set-DataRootAcls {
    param([Parameter(Mandatory = $true)][string]$DataRoot, [Parameter(Mandatory = $true)][string]$UserSid)
    $databaseFolder = Join-Path $DataRoot 'data'
    $logFolder = Join-Path $DataRoot 'logs'
    New-Item -ItemType Directory -Force -Path $DataRoot | Out-Null
    Invoke-Icacls @($DataRoot, '/inheritance:r', '/grant:r', '*S-1-5-18:(OI)(CI)F', '*S-1-5-32-544:(OI)(CI)F', "*${UserSid}:(RX)")
    New-Item -ItemType Directory -Force -Path $databaseFolder, $logFolder | Out-Null
    Invoke-Icacls @($databaseFolder, '/grant:r', "*${UserSid}:(OI)(CI)M")
    Invoke-Icacls @($logFolder, '/grant:r', "*${UserSid}:(OI)(CI)RX")
}
