# Measures what the INSTALLED SYNTRAC collector costs, idle and under a synthetic burst of activity.
#
#   scripts/dev/measure-overhead.ps1                      (30 s idle, then 60 s of load)
#   scripts/dev/measure-overhead.ps1 -IdleSeconds 60 -LoadSeconds 120
#
# Idle: the machine as it is. Load: every second, a burst of short-lived processes and loopback TCP connections, which
# is exactly what the collector records (process start/stop and network events). Reads ordinary performance counters and
# the collector's own status, so it needs NO elevation. It changes nothing except to create that activity (the extra
# rows land in the collector's database like any other activity).
#
# PASS/FAIL thresholds are parameters, not claims: the defaults are a starting budget to tune from real numbers.
param(
    [int]$IdleSeconds = 30,
    [int]$LoadSeconds = 60,
    [int]$SampleEverySeconds = 2,
    [int]$ProcessesPerSecond = 20,
    [int]$ConnectionsPerSecond = 50,
    [double]$MaxCpuPercentOfMachine = 2.0,
    [double]$MaxWorkingSetMB = 300
)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
$exe = Join-Path $env:ProgramFiles 'Syntrac\bin\syntrac-service.exe'
$database = Join-Path $env:ProgramData 'Syntrac\data\syntrac.db'
$logicalCpus = [Environment]::ProcessorCount
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$resultFile = Join-Path $root "aiChangeLog/overhead-$stamp.txt"
$lines = New-Object System.Collections.ArrayList

function Say {
    param([string]$Text, [string]$Colour = '')
    [void]$lines.Add($Text)
    if ($Colour) { Write-Host $Text -ForegroundColor $Colour } else { Write-Host $Text }
}

function Get-CollectorProcess {
    $p = Get-Process -Name 'syntrac-service' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $p) { throw 'The syntrac-service process is not running. Start the service first.' }
    return $p
}

# `--status` reports "ETW: N received, M DROPPED, ..." (the pipe is open to ordinary users).
function Get-EtwCounts {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $text = (& $exe --status 2>&1 | ForEach-Object { "$_" }) -join "`n" } finally { $ErrorActionPreference = $previous }
    if ($text -match 'ETW:\s+(\d+) received, (\d+) DROPPED') {
        return [pscustomobject]@{ Received = [int64]$Matches[1]; Dropped = [int64]$Matches[2] }
    }
    throw "Could not read the ETW counters from --status: $text"
}

function Get-DatabaseBytes {
    $total = 0
    foreach ($suffix in '', '-wal') {
        $item = Get-Item -LiteralPath ($database + $suffix) -ErrorAction SilentlyContinue
        if ($item) { $total += $item.Length }
    }
    return $total
}

# One reading of the collector's process counters. CPU is a share of ONE core in the counter; it is converted to a
# share of the whole machine, the unit SYNTRAC uses everywhere.
function Get-Sample {
    param([int]$ProcessId)
    $row = Get-CimInstance -ClassName Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess = $ProcessId"
    if (-not $row) { throw "No performance counters for process $ProcessId" }
    return [pscustomobject]@{
        CpuMachinePercent = [double]$row.PercentProcessorTime / $logicalCpus
        WorkingSetMB      = [double]$row.WorkingSet / 1MB
        Handles           = [int]$row.HandleCount
    }
}

function New-Burst {
    param([int]$Processes, [int]$Connections, $Listener, [int]$Port)
    for ($i = 0; $i -lt $Processes; $i++) {
        $info = New-Object System.Diagnostics.ProcessStartInfo
        $info.FileName = 'cmd.exe'
        $info.Arguments = '/c exit'
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $child = [System.Diagnostics.Process]::Start($info)
        $child.Dispose()
    }
    $payload = [Text.Encoding]::ASCII.GetBytes('x' * 512)
    for ($i = 0; $i -lt $Connections; $i++) {
        $client = New-Object System.Net.Sockets.TcpClient('127.0.0.1', $Port)
        $client.GetStream().Write($payload, 0, $payload.Length)
        $client.Close()
        # Accept what has queued up, or the listener's backlog fills and Windows starts refusing connections.
        while ($Listener.Pending()) { $Listener.AcceptTcpClient().Close() }
    }
    while ($Listener.Pending()) { $Listener.AcceptTcpClient().Close() }
}

# Runs one phase, sampling every few seconds, and returns the averages and peaks.
function Measure-Phase {
    param([string]$Name, [int]$Seconds, [bool]$Load, $Listener, [int]$Port)
    $collector = Get-CollectorProcess
    $etwBefore = Get-EtwCounts
    $dbBefore = Get-DatabaseBytes
    $samples = New-Object System.Collections.ArrayList
    $started = Get-Date
    $nextSample = $started
    while (((Get-Date) - $started).TotalSeconds -lt $Seconds) {
        $tick = Get-Date
        if ($Load) { New-Burst -Processes $ProcessesPerSecond -Connections $ConnectionsPerSecond -Listener $Listener -Port $Port }
        if ($tick -ge $nextSample) {
            [void]$samples.Add((Get-Sample -ProcessId $collector.Id))
            $nextSample = $tick.AddSeconds($SampleEverySeconds)
        }
        $spent = ((Get-Date) - $tick).TotalMilliseconds
        if ($spent -lt 1000) { Start-Sleep -Milliseconds (1000 - [int]$spent) }
    }
    $elapsed = ((Get-Date) - $started).TotalSeconds
    $etwAfter = Get-EtwCounts
    $dbAfter = Get-DatabaseBytes
    $cpu = $samples | ForEach-Object { $_.CpuMachinePercent }
    $mem = $samples | ForEach-Object { $_.WorkingSetMB }
    $handles = $samples | ForEach-Object { $_.Handles }
    return [pscustomobject]@{
        Name = $Name; Seconds = [math]::Round($elapsed, 1); Samples = $samples.Count
        CpuAvg = ($cpu | Measure-Object -Average).Average; CpuMax = ($cpu | Measure-Object -Maximum).Maximum
        MemMax = ($mem | Measure-Object -Maximum).Maximum; HandlesMax = ($handles | Measure-Object -Maximum).Maximum
        EtwPerSec = ($etwAfter.Received - $etwBefore.Received) / $elapsed
        EtwDropped = $etwAfter.Dropped - $etwBefore.Dropped
        DbGrowthKB = ($dbAfter - $dbBefore) / 1KB
        DbGrowthPerHourMB = (($dbAfter - $dbBefore) / 1MB) / $elapsed * 3600
    }
}

function Show-Phase {
    param($P)
    Say ("  {0,-5} {1,5} s   CPU avg {2,5:N2}% max {3,5:N2}% of machine   memory max {4,6:N1} MB   handles max {5}" -f $P.Name, $P.Seconds, $P.CpuAvg, $P.CpuMax, $P.MemMax, $P.HandlesMax)
    Say ("        ETW {0,7:N0} events/s   dropped {1}   database +{2:N0} KB  (= {3:N1} MB/hour at this rate)" -f $P.EtwPerSec, $P.EtwDropped, $P.DbGrowthKB, $P.DbGrowthPerHourMB)
}

$collector = Get-CollectorProcess
Say "SYNTRAC overhead measurement  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Say "Collector pid $($collector.Id), $logicalCpus logical CPUs, load = $ProcessesPerSecond processes + $ConnectionsPerSecond connections per second"
Say ''

$listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
try {
    Say "Idle for $IdleSeconds s..."
    $idle = Measure-Phase -Name 'idle' -Seconds $IdleSeconds -Load $false -Listener $listener -Port $port
    Say "Under load for $LoadSeconds s..."
    $load = Measure-Phase -Name 'load' -Seconds $LoadSeconds -Load $true -Listener $listener -Port $port
} finally {
    $listener.Stop()
}

Say ''
Say 'Results'
Show-Phase $idle
Show-Phase $load
Say ''

$failures = 0
function Judge {
    param([string]$What, [bool]$Ok, [string]$Detail)
    $label = 'FAIL'; $colour = 'Red'
    if ($Ok) { $label = 'PASS'; $colour = 'Green' } else { $script:failures++ }
    Say "[$label] $What - $Detail" $colour
}

Judge 'Collector keeps up: no ETW events dropped under load' ($load.EtwDropped -eq 0) "$($load.EtwDropped) dropped"
Judge "Collector CPU under load stays within budget (<= $MaxCpuPercentOfMachine% of the machine, average)" ($load.CpuAvg -le $MaxCpuPercentOfMachine) ("{0:N2}%" -f $load.CpuAvg)
Judge "Collector memory stays within budget (<= $MaxWorkingSetMB MB)" ($load.MemMax -le $MaxWorkingSetMB) ("{0:N1} MB" -f $load.MemMax)
Judge 'The load actually reached the collector (more events per second than idle)' ($load.EtwPerSec -gt ($idle.EtwPerSec * 2)) ("{0:N0} vs {1:N0} events/s" -f $load.EtwPerSec, $idle.EtwPerSec)

Say ''
if ($failures -eq 0) { Say 'overhead: ALL CHECKS PASSED' 'Green' } else { Say "overhead: $failures CHECK(S) FAILED" 'Red' }
Set-Content -Path $resultFile -Value $lines -Encoding utf8
Write-Host "Result file: $resultFile"
if ($failures -ne 0) { exit 1 }
exit 0
