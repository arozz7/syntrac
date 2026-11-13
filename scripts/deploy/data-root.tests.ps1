# Self-test for data-root.ps1 (run by verify.ps1). Exit code 0 = all passed.
# The decision about whether a folder is a safe place for the collector's data is pure: it is given facts about the
# volume, so every case (USB disk, network share, nearly full drive, ...) is tested without such hardware.
. (Join-Path $PSScriptRoot 'data-root.ps1')

$script:failures = 0
function Assert-That {
    param([string]$Name, [bool]$Condition)
    if ($Condition) { Write-Host "  ok   $Name" } else { Write-Host "  FAIL $Name" -ForegroundColor Red; $script:failures++ }
}

function New-Facts {
    param([bool]$Exists = $true, [string]$DriveType = 'Fixed', [string]$BusType = 'SATA', [int64]$FreeBytes = 263GB)
    return [pscustomobject]@{ Exists = $Exists; DriveType = $DriveType; BusType = $BusType; FreeBytes = $FreeBytes }
}

$min = 20GB
$good = Test-DataRootChoice -DataRoot 'D:\Syntrac' -Volume (New-Facts) -MinFreeBytes $min
Assert-That 'a fixed internal SSD with plenty of room is accepted' ($good.Ok -and $good.Problems.Count -eq 0)

Assert-That 'the drive root itself is accepted' ((Test-DataRootChoice -DataRoot 'O:\' -Volume (New-Facts) -MinFreeBytes $min).Ok)
Assert-That 'NVMe is accepted' ((Test-DataRootChoice -DataRoot 'D:\Syntrac' -Volume (New-Facts -BusType 'NVMe') -MinFreeBytes $min).Ok)
Assert-That 'a disk whose bus cannot be determined is not refused for that reason' ((Test-DataRootChoice -DataRoot 'D:\Syntrac' -Volume (New-Facts -BusType 'Unknown') -MinFreeBytes $min).Ok)

$usb = Test-DataRootChoice -DataRoot 'M:\Syntrac' -Volume (New-Facts -BusType 'USB') -MinFreeBytes $min
Assert-That 'a USB-connected disk is refused (it can be unplugged under a running service)' (-not $usb.Ok)
Assert-That 'the reason names USB' (($usb.Problems -join ' ') -match 'USB')
Assert-That 'a FireWire disk is refused too' (-not (Test-DataRootChoice -DataRoot 'M:\Syntrac' -Volume (New-Facts -BusType '1394') -MinFreeBytes $min).Ok)
Assert-That 'an iSCSI disk is refused too' (-not (Test-DataRootChoice -DataRoot 'M:\Syntrac' -Volume (New-Facts -BusType 'iSCSI') -MinFreeBytes $min).Ok)

Assert-That 'a removable drive is refused' (-not (Test-DataRootChoice -DataRoot 'E:\Syntrac' -Volume (New-Facts -DriveType 'Removable') -MinFreeBytes $min).Ok)
Assert-That 'a mapped network drive is refused' (-not (Test-DataRootChoice -DataRoot 'Z:\Syntrac' -Volume (New-Facts -DriveType 'Network') -MinFreeBytes $min).Ok)
Assert-That 'an optical drive is refused' (-not (Test-DataRootChoice -DataRoot 'K:\Syntrac' -Volume (New-Facts -DriveType 'CD-ROM') -MinFreeBytes $min).Ok)

$unc = Test-DataRootChoice -DataRoot '\\server\share\Syntrac' -Volume (New-Facts) -MinFreeBytes $min
Assert-That 'a network share (UNC path) is refused' (-not $unc.Ok)
Assert-That 'the UNC reason names the network' (($unc.Problems -join ' ') -match 'network')

$relative = Test-DataRootChoice -DataRoot 'Syntrac\data' -Volume (New-Facts) -MinFreeBytes $min
Assert-That 'a relative path is refused (the service starts in System32)' (-not $relative.Ok)
Assert-That 'the relative-path reason says absolute' (($relative.Problems -join ' ') -match 'absolute')

$missing = Test-DataRootChoice -DataRoot 'Q:\Syntrac' -Volume (New-Facts -Exists $false) -MinFreeBytes $min
Assert-That 'a drive that is not there is refused' (-not $missing.Ok)
Assert-That 'the missing-drive reason names the drive' (($missing.Problems -join ' ') -match 'Q:')

$low = Test-DataRootChoice -DataRoot 'D:\Syntrac' -Volume (New-Facts -FreeBytes 5GB) -MinFreeBytes $min
Assert-That 'a drive without enough free space is refused' (-not $low.Ok)
Assert-That 'the space reason gives the numbers' (($low.Problems -join ' ') -match '5(\.0)? GB' -and ($low.Problems -join ' ') -match '20(\.0)? GB')
Assert-That 'exactly the minimum free space is enough' ((Test-DataRootChoice -DataRoot 'D:\Syntrac' -Volume (New-Facts -FreeBytes $min) -MinFreeBytes $min).Ok)

$both = Test-DataRootChoice -DataRoot 'M:\Syntrac' -Volume (New-Facts -BusType 'USB' -FreeBytes 1GB) -MinFreeBytes $min
Assert-That 'every problem is reported together, not just the first' ($both.Problems.Count -eq 2)

Assert-That 'the default minimum free space is a sensible amount' ($script:DefaultMinFreeBytes -ge 10GB)

# --- proving a copy of the database is faithful (reports come from `syntrac-service --check-db`)
$original = @('database:  C:/ProgramData/Syntrac/data/syntrac.db', 'schema:    v3', 'integrity: ok', 'table events 83259', 'table flows 29629')
$copy = @('database:  D:/Syntrac/data/syntrac.db', 'schema:    v3', 'integrity: ok', 'table events 83259', 'table flows 29629')
$same = Compare-DatabaseReports -Source $original -Copy $copy
Assert-That 'a faithful copy is identical apart from its path' ($same.Identical -and $same.Differences.Count -eq 0)

$short = @('database:  O:/x.db', 'schema:    v3', 'integrity: ok', 'table events 83000', 'table flows 29629')
$lost = Compare-DatabaseReports -Source $original -Copy $short
Assert-That 'a copy with fewer rows is not identical' (-not $lost.Identical)
Assert-That 'the difference names the table' (($lost.Differences -join ' ') -match 'events')

$missingTable = @('database:  O:/x.db', 'schema:    v3', 'integrity: ok', 'table events 83259')
Assert-That 'a copy missing a table is not identical' (-not (Compare-DatabaseReports -Source $original -Copy $missingTable).Identical)

$otherVersion = @('database:  O:/x.db', 'schema:    v2', 'integrity: ok', 'table events 83259', 'table flows 29629')
Assert-That 'a copy at a different schema version is not identical' (-not (Compare-DatabaseReports -Source $original -Copy $otherVersion).Identical)

Assert-That 'a report whose integrity check passed is healthy' (Test-DatabaseReportHealthy $original)
Assert-That 'a report with a failed integrity check is not healthy' (-not (Test-DatabaseReportHealthy @('database:  x', 'schema:    v3', 'integrity: FAILED (Page 7 damaged)', 'table events 1')))
Assert-That 'an empty report is not healthy' (-not (Test-DatabaseReportHealthy @()))

if ($script:failures -gt 0) { Write-Host "$($script:failures) failed" -ForegroundColor Red; exit 1 }
Write-Host 'data root helpers: all passed'
exit 0
