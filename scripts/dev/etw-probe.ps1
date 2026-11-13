# Diagnostic: does Windows deliver the ETW events SYNTRAC depends on, independent of SYNTRAC's own code?
#
# 1. Prints buffer and loss statistics for EVERY running trace session (a session that is not being drained, or an
#    orphan left behind by a crashed run, shows up here).
# 2. Probes each provider in its OWN short session with generous buffers, so one chatty provider cannot drown the
#    others, and reports events captured and events lost per provider.
#
# Needs an elevated PowerShell. Deletes nothing and stops only its own "SyntracProbe" session. The probe files stay in
# the folder printed at the end.
#
#   powershell -ExecutionPolicy Bypass -File scripts/dev/etw-probe.ps1
$ErrorActionPreference = 'Continue'
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host 'This probe needs an elevated PowerShell (Run as administrator).' -ForegroundColor Red
    exit 2
}

$session = 'SyntracProbe'
$folder = Join-Path $env:TEMP ("syntrac-etw-probe-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $folder | Out-Null
Write-Host "Probe files: $folder"

Write-Host ''
Write-Host '== 1. Every running trace session: buffers, free buffers, events lost =='
try {
    Get-EtwTraceSession -Name '*' -ErrorAction Stop |
        Select-Object Name, NumberOfBuffers, FreeBuffers, EventsLost, BuffersWritten, RealTimeBuffersLost, MaximumBuffers |
        Sort-Object EventsLost -Descending | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
} catch {
    Write-Host "  (could not read session statistics: $($_.Exception.Message))"
}
Write-Host 'Sessions whose name contains Syntrac (the console one should NOT be listed unless a console collector runs):'
logman query -ets | Select-String -Pattern 'Syntrac' | ForEach-Object { Write-Host "  $($_.Line)" }

# ---- activity generators: one per provider
function New-ProcessActivity { 1..15 | ForEach-Object { Start-Process -FilePath cmd.exe -ArgumentList '/c', 'exit' -WindowStyle Hidden -Wait } }
function New-NetworkActivity {
    $listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
    1..15 | ForEach-Object {
        $client = New-Object Net.Sockets.TcpClient('127.0.0.1', $port)
        $bytes = [Text.Encoding]::ASCII.GetBytes('x' * 1024)
        $client.GetStream().Write($bytes, 0, $bytes.Length)
        $client.Close()
    }
    $listener.Stop()
}
function New-DnsActivity {
    1..5 | ForEach-Object {
        try { Resolve-DnsName -Name "example$_.com" -Type A -DnsOnly -ErrorAction Stop | Out-Null } catch { }
    }
}

function Test-Provider {
    param([string]$Label, [string]$Provider, [string]$Keywords, [scriptblock]$Generate)
    $etl = Join-Path $folder "$Label.etl"
    $summary = Join-Path $folder "$Label-summary.txt"
    logman stop $session -ets 2>&1 | Out-Null      # only ever our own probe session
    # 64 KB buffers, 16-128 of them: far more headroom than the defaults.
    logman start $session -ets -o $etl -bs 64 -nb 16 128 -p $Provider $Keywords 0xff 2>&1 | Out-Null
    & $Generate
    Start-Sleep -Seconds 3
    logman stop $session -ets 2>&1 | Out-Null
    tracerpt $etl -of CSV -o (Join-Path $folder "$Label.csv") -summary $summary -y 2>&1 | Out-Null
    $processed = '?'; $lost = '?'; $rows = 0
    if (Test-Path $summary) {
        $text = Get-Content $summary
        $p = $text | Select-String -Pattern 'Total Events\s+Processed\s+(\d+)'
        $l = $text | Select-String -Pattern 'Total Events\s+Lost\s+(\d+)'
        if ($p) { $processed = $p.Matches[0].Groups[1].Value }
        if ($l) { $lost = $l.Matches[0].Groups[1].Value }
    }
    # Events other than the two session header records are the ones that matter.
    $payload = '?'
    if ($processed -match '^\d+$') { $payload = [Math]::Max(0, [int]$processed - 2) }
    $verdict = 'NO EVENTS'
    if ($payload -is [int] -and $payload -gt 0) { $verdict = 'delivering' }
    Write-Host ("  {0,-16} provider events: {1,-6} total processed: {2,-6} lost: {3,-6} => {4}" -f $Label, $payload, $processed, $lost, $verdict)
}

Write-Host ''
Write-Host '== 2. One provider per session (each with its own generated activity) =='
Test-Provider 'kernel-process' 'Microsoft-Windows-Kernel-Process' '0x10' { New-ProcessActivity }
Test-Provider 'kernel-network' 'Microsoft-Windows-Kernel-Network' '0xffffffffffffffff' { New-NetworkActivity }
Test-Provider 'dns-client'     'Microsoft-Windows-DNS-Client'     '0xffffffffffffffff' { New-DnsActivity }

Write-Host ''
Write-Host 'How to read this:'
Write-Host '  - Providers "delivering": Windows ETW works, so the fault is in how the SYNTRAC session is set up or drained.'
Write-Host '  - Every provider "NO EVENTS" with lost > 0: ETW cannot allocate buffers system-wide. Look at section 1 for a'
Write-Host '    session with many lost events or no free buffers, stop THAT session (your own orphans only), then retry.'
Write-Host '  - Every provider "NO EVENTS" with lost = 0: nothing is being generated at all; reboot.'
Write-Host "Probe files kept in: $folder"
