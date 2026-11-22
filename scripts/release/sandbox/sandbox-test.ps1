# Runs INSIDE Windows Sandbox (started by run-installer-test.ps1 through the .wsb file): drives the real installer and checks what it does.
# Compatible with Windows PowerShell 5.1. Writes C:\Results\report.txt, one PASS/FAIL line per check, then 'DONE'.
#
# Folders (mapped by the launcher): C:\SetupTest\good and C:\SetupTest\broken hold the two installers, C:\Results is where the report goes.
$ErrorActionPreference = 'Continue'
$good = (Get-ChildItem 'C:\SetupTest\good\*_x64-setup.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
$broken = (Get-ChildItem 'C:\SetupTest\broken\*_x64-setup*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
$report = 'C:\Results\report.txt'
$installDir = 'C:\Program Files\SYNTRAC'
$serviceExe = Join-Path $installDir 'bin\syntrac-service.exe'
$configFile = Join-Path $installDir 'config\service.toml'
$database = 'C:\ProgramData\Syntrac\data\syntrac.db'
$setupLog = Join-Path $env:TEMP 'SYNTRAC-setup.log'
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$script:failed = 0

function Say { param([string]$Text) Add-Content -Path $report -Value $Text; Write-Host $Text }
function Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = '')
    if ($Ok) { Say "PASS  $Name" } else { Say "FAIL  $Name  $Detail"; $script:failed++ }
}
function Get-State { $s = Get-Service -Name Syntrac -ErrorAction SilentlyContinue; if ($s) { return $s.Status.ToString() } return 'missing' }
function Wait-For {
    param([scriptblock]$Condition, [int]$Seconds = 90)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) { if (& $Condition) { return $true }; Start-Sleep -Seconds 1 }
    return $false
}
function Hash { param([string]$Path) if (Test-Path $Path) { return (Get-FileHash -Algorithm SHA256 $Path).Hash } return 'absent' }
# The installer's logs live in this sandbox's temp folder; a copy goes to the results folder after every install so a failure can be read on the host.
$script:installs = 0
function Save-Logs {
    $script:installs++
    foreach ($log in Get-ChildItem (Join-Path $env:TEMP 'SYNTRAC-*.log') -ErrorAction SilentlyContinue) {
        Copy-Item $log.FullName ("C:\Results\step{0}-{1}" -f $script:installs, $log.Name) -Force
    }
}
function Install {
    param([string]$Installer, [string[]]$More = @())
    $code = (Start-Process -FilePath $Installer -ArgumentList (@('/S', "/USERSID=$sid") + $More) -Wait -PassThru).ExitCode
    Save-Logs
    return $code
}

Set-Content -Path $report -Value "SYNTRAC installer test, $(Get-Date -Format s), user $sid"

# 1. First install.
$code = Install $good
Check 'first install exits 0' ($code -eq 0) "exit code $code"
Check 'the service is running' (Wait-For { (Get-State) -eq 'Running' }) (Get-State)
Check 'the app is installed' (Test-Path (Join-Path $installDir 'syntrac-app.exe'))
Check 'the service program is installed' (Test-Path $serviceExe)
Check 'the config names the user who runs the app' ((Test-Path $configFile) -and ((Get-Content $configFile -Raw) -match [regex]::Escape($sid)))
Check 'the database appears' (Wait-For { Test-Path $database } 60)
$ping = & $serviceExe --ping 2>&1
Check 'the user can reach the service over its pipe' ("$ping" -match 'pong') "$ping"

# 2. Installing the same version again (the silent path of an upgrade): config and data stay, the service runs.
$configBefore = Hash $configFile
$exeBefore = Hash $serviceExe
$code = Install $good
Check 'a second install exits 0' ($code -eq 0) "exit code $code"
Check 'the service is running again' (Wait-For { (Get-State) -eq 'Running' }) (Get-State)
Check 'the config was kept as it was' ((Hash $configFile) -eq $configBefore)
Check 'the database is still there' (Test-Path $database)
Check 'the previous program was kept for a rollback' (Test-Path "$serviceExe.previous")

# 2b. An upgrade while the app is open: the silent installer closes it, then finishes as usual.
# The schema line shows the quick read of the database's schema was used (the full check takes minutes on a large database).
Check 'the install log says which schema the database has' ((Test-Path $setupLog) -and ((Get-Content $setupLog -Raw) -match 'Database: schema v\d+'))
$appExe = Join-Path $installDir 'syntrac-app.exe'
$appProcess = Start-Process -FilePath $appExe -PassThru
Check 'the app is running before the upgrade' (-not $appProcess.HasExited)
Start-Sleep -Seconds 5
$code = Install $good
Check 'an upgrade with the app open exits 0' ($code -eq 0) "exit code $code"
Check 'the open app was closed by the installer' (Wait-For { $appProcess.HasExited } 30)
Check 'the service is running after it' (Wait-For { (Get-State) -eq 'Running' }) (Get-State)

# 3. An installer replacing this version runs the old uninstaller in place: that must leave the service alone.
Start-Process -FilePath (Join-Path $installDir 'uninstall.exe') -ArgumentList '/S', "_?=$installDir" -Wait | Out-Null
Check 'the in-place uninstall of an upgrade leaves the service running' ((Get-State) -eq 'Running') (Get-State)
Check 'it leaves the service program and config' ((Test-Path $serviceExe) -and (Test-Path $configFile))

# 4. An upgrade whose service cannot start: the installer fails and the old service is put back.
$exeBefore = Hash $serviceExe
$code = Install $broken
Check 'the failing upgrade exits non-zero' ($code -ne 0) "exit code $code"
Check 'the old service is running again' (Wait-For { (Get-State) -eq 'Running' }) (Get-State)
Check 'the old program is back' ((Hash $serviceExe) -eq $exeBefore)
Check 'the config is as it was' ((Hash $configFile) -eq $configBefore)
Check 'the log says what was put back' ((Test-Path $setupLog) -and ((Get-Content $setupLog -Raw) -match 'Rollback finished'))

# 5. A folder outside Program Files is refused before anything is installed.
$code = Install $good @('/D=C:\SetupTest\Elsewhere')
Check 'an install outside Program Files exits non-zero' ($code -ne 0) "exit code $code"
Check 'nothing was put in that folder' (-not (Test-Path 'C:\SetupTest\Elsewhere\bin'))
Check 'the service in use was not touched' ((Get-State) -eq 'Running')

# 6. A real uninstall removes the service and the program and keeps the config and the data.
$code = Install $good
Check 'the install before the uninstall exits 0' ($code -eq 0) "exit code $code"
Start-Process -FilePath (Join-Path $installDir 'uninstall.exe') -ArgumentList '/S' -Wait | Out-Null
# The uninstaller copies itself aside and the first process returns at once, so wait for the result.
Check 'the service is removed' (Wait-For { (Get-State) -eq 'missing' } 120) (Get-State)
Check 'the app is removed' (Wait-For { -not (Test-Path (Join-Path $installDir 'syntrac-app.exe')) } 60)
Check 'the program folder (bin) is removed' (-not (Test-Path (Join-Path $installDir 'bin')))
Check 'the config is kept' (Test-Path $configFile)
Check 'the data is kept' (Test-Path $database)

Say ''
if ($script:failed -eq 0) { Say 'ALL CHECKS PASSED' } else { Say "$($script:failed) CHECK(S) FAILED" }
Say 'DONE'
