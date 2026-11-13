# End-to-end check of the installed service, from the point of view of the ordinary (unelevated) user.
# Compatible with Windows PowerShell 5.1.
#
#   powershell -ExecutionPolicy Bypass -File scripts/deploy/smoke-test-service.ps1 [-Uninstall]
#
# One UAC prompt. The elevated half installs and starts the service, later stops and restarts it; everything in
# between is checked HERE, unelevated, because only that vantage point proves the permissions are right:
#   * the database opens read-only and holds fresh data (needs the user's Modify right on the data folder so
#     SQLite can use the WAL "-shm" file that SYSTEM created)
#   * the user can read the logs but cannot write to the program or config folders
#   * the service stops within 30 s, restarts, and writes data again
#   * -Uninstall: the registration disappears but the data stays
# Nothing is deleted by this script. Results: aiChangeLog/service-smoke-<stamp>.txt (PASS/FAIL per check).
param(
    # Where the service keeps its data. Default: wherever the installed config already points (so the test follows a
    # relocated data root), else the standard location. Pass a path to test a different root, e.g. D:\Syntrac.
    [string]$DataDir,
    [switch]$Uninstall,
    [int]$FreshWithinSeconds = 30,
    [int]$MaxStopSeconds = 30
)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
Set-Location $root

. (Join-Path $PSScriptRoot '../dev/use-cargo.ps1')
if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
    throw 'python was not found on PATH. The smoke test reads the database through Python (read-only), as the UI will.'
}
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host 'Run this from a NORMAL (unelevated) PowerShell: the point is to test what an ordinary user can do.' -ForegroundColor Red
    exit 2
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$runDir = Join-Path $root "data/smoke-$stamp"
$resultFile = Join-Path $root "aiChangeLog/service-smoke-$stamp.txt"
New-Item -ItemType Directory -Force -Path $runDir | Out-Null

$installDir = Join-Path $env:ProgramFiles 'Syntrac'
# (PowerShell variable names are case-insensitive: $DataDir is the parameter above; there is no separate local.)
. (Join-Path $PSScriptRoot 'config-helpers.ps1')
$installedConfig = Join-Path $installDir 'config/service.toml'
if (-not $DataDir -and (Test-Path $installedConfig)) { $DataDir = Get-DataRootFromConfig (Get-Content -Path $installedConfig -Raw) }
if (-not $DataDir) { $DataDir = Join-Path $env:ProgramData 'Syntrac' }
Write-Host "Data root under test: $DataDir"
$database = Join-Path $DataDir 'data/syntrac.db'
$logDir = Join-Path $DataDir 'logs'
$checker = Join-Path $PSScriptRoot 'check_db.py'

$results = New-Object System.Collections.ArrayList
function Add-Result {
    param([string]$Name, [bool]$Pass, [string]$Detail = '')
    $label = 'FAIL'
    if ($Pass) { $label = 'PASS' }
    $line = "[$label] $Name"
    if ($Detail) { $line += " - $Detail" }
    [void]$results.Add($line)
    $colour = 'Red'
    if ($Pass) { $colour = 'Green' }
    Write-Host $line -ForegroundColor $colour
}

function Wait-Marker {
    param([string]$Name, [int]$Seconds)
    $path = Join-Path $runDir $Name
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $path) { return $true }
        if (Test-Path (Join-Path $runDir 'error.txt')) { return $false }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

# Reads the database the way the UI will: read-only, unelevated. Returns $null when it cannot.
function Read-Database {
    $output = & python -I $checker $database 2>&1
    if ($LASTEXITCODE -ne 0) { return $null }
    $parts = ("$output").Trim().Split(' ')
    return [pscustomobject]@{
        MetricsMicros = [int64]$parts[0]; EventsMicros = [int64]$parts[1]; Version = [int]$parts[2]
        Running = [int]$parts[3]; Approximate = [int]$parts[4]
    }
}

function Get-AgeSeconds {
    param([int64]$Micros)
    $nowMicros = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() * 1000
    return [math]::Round(($nowMicros - $Micros) / 1000000, 1)
}

function Wait-FreshData {
    param([int]$Seconds)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $snapshot = $null
        try { $snapshot = Read-Database } catch { $snapshot = $null }
        if ($snapshot -and $snapshot.MetricsMicros -gt 0) {
            $age = Get-AgeSeconds $snapshot.MetricsMicros
            if ($age -lt $FreshWithinSeconds) { return $snapshot }
        }
        Start-Sleep -Seconds 2
    }
    return $null
}

# SID first, while still the ordinary user. The elevated session may belong to another administrator account.
$userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
Write-Host "Testing as $([Security.Principal.WindowsIdentity]::GetCurrent().Name) ($userSid)"

Write-Host 'Building the release binary...'
& cargo build --release -p syntrac-service
if ($LASTEXITCODE -ne 0) { Write-Host 'Build failed' -ForegroundColor Red; exit 1 }
$exe = Join-Path $root 'target/release/syntrac-service.exe'

# Runs the collector executable and returns its exit code and combined output. Windows PowerShell 5.1 turns ANY text
# a native program writes to stderr into a terminating error when $ErrorActionPreference is Stop, and "not running"
# is reported on stderr on purpose, so the preference is relaxed just for the call.
function Invoke-Collector {
    param([string[]]$Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = (& $exe @Arguments 2>&1 | ForEach-Object { "$_" }) -join "`n"
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [pscustomobject]@{ ExitCode = $code; Output = $output.Trim() }
}

# A trace that "started" but delivers nothing leaves every other number healthy (metrics keep flowing), which is
# exactly how a dead ETW session went unnoticed for over an hour. So prove events actually arrive: on a normal
# machine something starts or talks to the network within seconds.
function Wait-EtwDelivering {
    param([int]$Seconds)
    $deadline = (Get-Date).AddSeconds($Seconds)
    $received = -1
    while ((Get-Date) -lt $deadline) {
        $status = Invoke-Collector @('--status')
        if ($status.Output -match 'ETW:\s+(\d+) received') {
            $received = [int64]$Matches[1]
            if ($received -gt 0) { return $received }
        }
        Start-Sleep -Seconds 3
    }
    return $received
}

$helper = Join-Path $PSScriptRoot 'smoke-elevated.ps1'
$helperArgs = "-NoProfile -ExecutionPolicy Bypass -File `"$helper`" -UserSid $userSid -RunDir `"$runDir`" -Executable `"$exe`" -DataDir `"$DataDir`""
if ($Uninstall) { $helperArgs += ' -Uninstall' }
Write-Host 'Starting the elevated installer (approve the UAC prompt)...'
Start-Process powershell -Verb RunAs -ArgumentList $helperArgs

$installed = Wait-Marker 'phase1.ready' 300
Add-Result 'Service installed and started (elevated half)' $installed
if (-not $installed) {
    $reason = ''
    if (Test-Path (Join-Path $runDir 'error.txt')) { $reason = Get-Content (Join-Path $runDir 'error.txt') -Raw }
    Add-Result 'Installer finished without error' $false $reason
}

if ($installed) {
    # --- checks as the ordinary user ---
    $service = Get-Service -Name 'Syntrac' -ErrorAction SilentlyContinue
    Add-Result 'Service is Running' ($service -and $service.Status -eq 'Running') "$($service.Status)"

    $fresh = Wait-FreshData 60
    Add-Result "Unelevated user reads the database read-only and sees data fresher than $FreshWithinSeconds s" ($null -ne $fresh) $database
    if ($fresh) {
        Add-Result 'Schema is current (a mismatch would block the UI)' ($fresh.Version -ge 3) "version $($fresh.Version)"
        Add-Result 'Running-process list is plausible after the startup snapshot' (($fresh.Running -gt 20) -and ($fresh.Running -lt 5000)) "$($fresh.Running) running"
    }

    # The status pipe: an ordinary user must be let in by its access list (SYSTEM created it).
    $status = Invoke-Collector @('--status')
    Add-Result 'Ordinary user can read the collector status over the pipe' (($status.ExitCode -eq 0) -and $status.Output.Contains('SYNTRAC collector')) ($status.Output -split "`n" | Select-Object -First 1)
    Write-Host $status.Output

    # The orphan guard runs when the service starts. It must have run (and been able to LIST sessions as SYSTEM),
    # or a leftover session could silently starve ETW again. Look at the newest log file.
    $newestLog = Get-ChildItem -Path $logDir -Filter '*.log' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
    $guardLines = @()
    if ($newestLog) { $guardLines = @(Get-Content $newestLog.FullName -ErrorAction SilentlyContinue | Select-String -Pattern 'leftover SYNTRAC|No leftover SYNTRAC|Could not list trace sessions' | Select-Object -Last 3 | ForEach-Object { $_.Line }) }
    $guardRan = ($guardLines.Count -gt 0) -and -not ($guardLines[-1] -match 'Could not list trace sessions')
    Add-Result 'Orphan guard ran at start and could list the trace sessions' $guardRan ($guardLines -join ' | ')
    $etwReceived = Wait-EtwDelivering 60
    Add-Result 'ETW delivers events (a started but silent trace would pass every other check)' ($etwReceived -gt 0) "$etwReceived events received within 60 s"

    $logs =@(Get-ChildItem -Path $logDir -Filter '*.log' -ErrorAction SilentlyContinue)
    Add-Result 'User can read a non-empty service log' (($logs.Count -gt 0) -and (($logs | Measure-Object -Property Length -Sum).Sum -gt 0)) "$($logs.Count) file(s) in $logDir"

    # If a probe write ever SUCCEEDS the permissions are wrong. The probe is not deleted (this script never
    # deletes anything); its path is reported so it can be removed by hand after fixing the folder permissions.
    foreach ($probeDir in (Join-Path $installDir 'bin'), (Join-Path $installDir 'config'), $logDir) {
        $probe = Join-Path $probeDir 'smoke-probe.txt'
        $wrote = $false
        try { New-Item -ItemType File -Path $probe -ErrorAction Stop | Out-Null; $wrote = $true } catch { $wrote = $false }
        $detail = ''
        if ($wrote) { $detail = "a probe file was created at $probe - remove it by hand and tighten the folder permissions" }
        Add-Result "User CANNOT write into $probeDir (privilege-escalation guard)" (-not $wrote) $detail
    }

    # --- stop / restart (elevated half does the work) ---
    Set-Content -Path (Join-Path $runDir 'phase2.go') -Value 'go' -Encoding ascii
    if (Wait-Marker 'phase2.done' 240) {
        $stop = (Get-Content (Join-Path $runDir 'phase2.done') -Raw).Trim()
        $seconds = [double](($stop -split ';')[0] -split '=')[1]
        $left = [int](($stop -split ';')[1] -split '=')[1]
        Add-Result "Service stops within $MaxStopSeconds s" ($seconds -le $MaxStopSeconds) "$seconds s"
        Add-Result 'No collector process is left running after the stop' ($left -eq 0)
        Add-Result 'Data survives the stop' (Test-Path $database)
        $stoppedPing = Invoke-Collector @('--ping')
        Add-Result 'The pipe is gone once the service has stopped (--ping reports not running)' ($stoppedPing.ExitCode -eq 4) "exit code $($stoppedPing.ExitCode)"
        # The stopped state has been inspected; the elevated side may now restart the service.
        Set-Content -Path (Join-Path $runDir 'phase2.ack') -Value 'ack' -Encoding ascii
    } else {
        Add-Result 'Service stop completed' $false 'phase2 did not finish'
    }

    if (Wait-Marker 'phase3.done' 180) {
        $service = Get-Service -Name 'Syntrac' -ErrorAction SilentlyContinue
        Add-Result 'Service is Running again after the restart' ($service -and $service.Status -eq 'Running')
        $again = Wait-FreshData 60
        Add-Result 'Collector writes fresh data again after the restart' ($null -ne $again)
        $restartedPing = Invoke-Collector @('--ping')
        Add-Result 'The pipe answers again after the restart' (($restartedPing.ExitCode -eq 0) -and $restartedPing.Output.Contains('pong')) $restartedPing.Output
        $etwAfterRestart = Wait-EtwDelivering 60
        Add-Result 'ETW delivers events again after the restart' ($etwAfterRestart -gt 0) "$etwAfterRestart events received within 60 s"
        if ($again) { Write-Host "    (processes closed as exited-while-down at the restart: $($again.Approximate))" }
    } else {
        Add-Result 'Service restarted' $false 'phase3 did not finish'
    }

    if ($Uninstall) {
        Set-Content -Path (Join-Path $runDir 'phase4.go') -Value 'go' -Encoding ascii
        if (Wait-Marker 'phase4.done' 180) {
            $gone = $null -eq (Get-Service -Name 'Syntrac' -ErrorAction SilentlyContinue)
            Add-Result 'Uninstall removes the service registration' $gone
            Add-Result 'Uninstall KEEPS the database' (Test-Path $database)
        } else {
            Add-Result 'Uninstall completed' $false 'phase4 did not finish'
        }
    }
}

[void](Wait-Marker 'elevated.done' 30)

$failures = @($results | Where-Object { $_ -like '[[]FAIL]*' }).Count
$summary = @("SYNTRAC service smoke test  $stamp", "user: $([Security.Principal.WindowsIdentity]::GetCurrent().Name)", '') + $results + @('', "$($results.Count - $failures) passed, $failures failed")
$summary | Set-Content -Path $resultFile -Encoding utf8
Write-Host ''
Write-Host "Result file: $resultFile"
Write-Host "Elevated log: $(Join-Path $runDir 'elevated.log')"
if ($failures -gt 0) { exit 1 } else { exit 0 }