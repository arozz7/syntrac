# Self-test for installer-helpers.ps1 (run by verify.ps1). Exit code 0 = all passed.
# Every decision the installer makes is pure: it is given facts (the config text, sizes, versions) so each case is tested without installing anything.
. (Join-Path $PSScriptRoot 'config-helpers.ps1')
. (Join-Path $PSScriptRoot 'installer-helpers.ps1')

$script:failures = 0
function Assert-That {
    param([string]$Name, [bool]$Condition)
    if ($Condition) { Write-Host "  ok   $Name" } else { Write-Host "  FAIL $Name" -ForegroundColor Red; $script:failures++ }
}
function Throws {
    param([scriptblock]$Block)
    try { & $Block | Out-Null; return $false } catch { return $true }
}

$alice = 'S-1-5-21-1-2-3-1001'
$admin = 'S-1-5-21-1-2-3-500'
$bob = 'S-1-5-21-1-2-3-1002'
$configOfAlice = "pipe_allowed_sid = `"$alice`"`r`nretention_days = 7`r`n"

# --- who reads the data (the service's pipe is opened to this one user)
$chosen = Resolve-ReadingUserSid -Override $bob -InstalledConfigText $configOfAlice -ConsoleUserSid $admin
Assert-That 'an explicit user wins over everything' (($chosen.Sid -eq $bob) -and ($chosen.Source -eq 'override'))
$chosen = Resolve-ReadingUserSid -InstalledConfigText $configOfAlice -ConsoleUserSid $admin
Assert-That 'an upgrade keeps the user already in the installed config, even when another administrator runs it' (($chosen.Sid -eq $alice) -and ($chosen.Source -eq 'installed config'))
$chosen = Resolve-ReadingUserSid -InstalledConfigText '' -ConsoleUserSid $bob
Assert-That 'a first install uses the signed-in desktop user' (($chosen.Sid -eq $bob) -and ($chosen.Source -eq 'desktop user'))
$chosen = Resolve-ReadingUserSid -InstalledConfigText 'pipe_allowed_sid = "nonsense"' -ConsoleUserSid $bob
Assert-That 'an unreadable user in the installed config is ignored, not trusted' ($chosen.Sid -eq $bob)
Assert-That 'with no way to know the user it refuses instead of guessing' (Throws { Resolve-ReadingUserSid -InstalledConfigText '' -ConsoleUserSid $null })
Assert-That 'a bad explicit user is refused, not replaced by another' (Throws { Resolve-ReadingUserSid -Override 'not-a-sid' -InstalledConfigText $configOfAlice -ConsoleUserSid $bob })
Assert-That 'the SYSTEM account is never the reading user' (Throws { Resolve-ReadingUserSid -Override 'S-1-5-18' -InstalledConfigText '' -ConsoleUserSid $null })

# --- where the program may go: the service runs as SYSTEM, so its folder must not be writable by ordinary users
Assert-That 'Program Files is accepted' (Test-InstallDirAcceptable -InstallDir 'C:\Program Files\SYNTRAC' -ProgramFiles 'C:\Program Files')
Assert-That 'the case of the folder does not matter' (Test-InstallDirAcceptable -InstallDir 'c:\program files\syntrac' -ProgramFiles 'C:\Program Files')
Assert-That 'a trailing slash does not matter' (Test-InstallDirAcceptable -InstallDir 'C:\Program Files\SYNTRAC\' -ProgramFiles 'C:\Program Files')
Assert-That 'another drive is refused' (-not (Test-InstallDirAcceptable -InstallDir 'D:\Apps\SYNTRAC' -ProgramFiles 'C:\Program Files'))
Assert-That 'the user profile is refused' (-not (Test-InstallDirAcceptable -InstallDir 'C:\Users\alice\AppData\Local\SYNTRAC' -ProgramFiles 'C:\Program Files'))
Assert-That 'a folder that only starts with the same letters is refused' (-not (Test-InstallDirAcceptable -InstallDir 'C:\Program Files Extra\SYNTRAC' -ProgramFiles 'C:\Program Files'))
Assert-That 'a way out with .. is refused' (-not (Test-InstallDirAcceptable -InstallDir 'C:\Program Files\..\Users\SYNTRAC' -ProgramFiles 'C:\Program Files'))
Assert-That 'Program Files itself is refused (it needs its own folder)' (-not (Test-InstallDirAcceptable -InstallDir 'C:\Program Files' -ProgramFiles 'C:\Program Files'))
Assert-That 'Program Files (x86) is refused when 64-bit Program Files is asked for' (-not (Test-InstallDirAcceptable -InstallDir 'C:\Program Files (x86)\SYNTRAC' -ProgramFiles 'C:\Program Files'))

# --- reading a schema number out of what the service prints
Assert-That 'the schema is read from the --version report' ((Read-SchemaVersion @('syntrac-service 0.2.0', 'schema:    v15')) -eq 15)
Assert-That 'the schema is read from a --check-db report' ((Read-SchemaVersion @('database:  D:\x.db', 'schema:    v9', 'integrity: ok')) -eq 9)
Assert-That 'no schema line gives nothing' ($null -eq (Read-SchemaVersion @('nothing here')))

# --- does an upgrade need a copy of the database, and may it go on
$GB = 1GB
$plan = Get-UpgradePlan -ShippedSchema 15 -DatabaseSchema 15 -DatabaseBytes (2 * $GB) -FreeBytes (1 * $GB)
Assert-That 'the same schema needs no copy and no free space' ($plan.CanProceed -and -not $plan.NeedsDatabaseCopy)
$plan = Get-UpgradePlan -ShippedSchema 16 -DatabaseSchema 15 -DatabaseBytes (2 * $GB) -FreeBytes (10 * $GB)
Assert-That 'a newer schema takes a copy first, so a rollback can put the data back' ($plan.CanProceed -and $plan.NeedsDatabaseCopy)
$plan = Get-UpgradePlan -ShippedSchema 16 -DatabaseSchema 15 -DatabaseBytes (2 * $GB) -FreeBytes (2 * $GB)
Assert-That 'without room for the copy the upgrade does not start' ((-not $plan.CanProceed) -and ($plan.Reason -like '*free*'))
$plan = Get-UpgradePlan -ShippedSchema 14 -DatabaseSchema 15 -DatabaseBytes (2 * $GB) -FreeBytes (10 * $GB)
Assert-That 'a database newer than this version is not opened by it' ((-not $plan.CanProceed) -and ($plan.Reason -like '*newer*'))
$plan = Get-UpgradePlan -ShippedSchema 16 -DatabaseSchema $null -DatabaseBytes 0 -FreeBytes 0
Assert-That 'a first install has no database to copy' ($plan.CanProceed -and -not $plan.NeedsDatabaseCopy)
$plan = Get-UpgradePlan -ShippedSchema $null -DatabaseSchema 15 -DatabaseBytes 100 -FreeBytes (10 * $GB)
Assert-That 'a new version that cannot say its schema is not installed over a database' (-not $plan.CanProceed)

# --- what a rollback does
$steps = @(Get-RollbackSteps -HadPreviousProgram $true -HadConfigCopy $true -HadDatabaseCopy $true -WasRunning $true)
Assert-That 'a full rollback restores the program, the config and the data, then starts the old service' (($steps -join ',') -eq 'stop-service,restore-program,restore-config,restore-database,start-service')
$steps = @(Get-RollbackSteps -HadPreviousProgram $true -HadConfigCopy $false -HadDatabaseCopy $false -WasRunning $false)
Assert-That 'only what was saved is restored, and a service that was stopped stays stopped' (($steps -join ',') -eq 'stop-service,restore-program')
$steps = @(Get-RollbackSteps -HadPreviousProgram $false -HadConfigCopy $false -HadDatabaseCopy $false -WasRunning $false)
Assert-That 'after a failed first install the half-made service is removed' (($steps -join ',') -eq 'stop-service,remove-service')

if ($script:failures -gt 0) { Write-Host "$($script:failures) failed" -ForegroundColor Red; exit 1 }
Write-Host 'installer-helpers: all passed'
exit 0
