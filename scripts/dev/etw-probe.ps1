# Diagnostic: does Windows deliver the ETW events SYNTRAC depends on, independent of SYNTRAC's own code?
# Starts a separate trace session ("SyntracProbe") on the three providers, generates a little activity, stops the
# session and counts the events per provider with tracerpt. Needs an elevated PowerShell. It deletes nothing;
# the probe files stay in the folder printed at the end.
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
$etl = Join-Path $folder 'probe.etl'

Write-Host "Probe files: $folder"
Write-Host ''
Write-Host '== Trace sessions visible now (SYNTRAC ones should be listed if running) =='
logman query -ets | Select-String -Pattern 'Syntrac|Data Collector Set|^-+' | ForEach-Object { $_.Line }
Write-Host ''
Write-Host '== SyntracCollector session details =='
logman query SyntracCollector -ets 2>&1 | ForEach-Object { $_ }

# Leftover from an earlier probe? Stop it first (this only ever touches the probe's own session name).
logman stop $session -ets 2>&1 | Out-Null

Write-Host ''
Write-Host '== Starting the probe session =='
logman start $session -ets -o $etl -p 'Microsoft-Windows-Kernel-Network' 0xffffffffffffffff 0xff | Out-Null
logman update $session -ets -p 'Microsoft-Windows-Kernel-Process' 0x10 0xff | Out-Null
logman update $session -ets -p 'Microsoft-Windows-DNS-Client' 0xffffffffffffffff 0xff | Out-Null

Write-Host '== Generating activity (processes, loopback TCP, DNS) for a few seconds =='
1..10 | ForEach-Object { Start-Process -FilePath cmd.exe -ArgumentList '/c', 'exit' -WindowStyle Hidden -Wait }
$listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
1..10 | ForEach-Object {
    $client = New-Object Net.Sockets.TcpClient('127.0.0.1', $port)
    $bytes = [Text.Encoding]::ASCII.GetBytes('x' * 1024)
    $client.GetStream().Write($bytes, 0, $bytes.Length)
    $client.Close()
}
$listener.Stop()
try { Resolve-DnsName -Name 'example.com' -Type A -DnsOnly -ErrorAction Stop | Out-Null } catch { Write-Host "  (DNS lookup failed: $($_.Exception.Message))" }
Start-Sleep -Seconds 3

logman stop $session -ets | Out-Null
Write-Host ''
Write-Host '== Events captured, per provider (tracerpt summary) =='
$summary = Join-Path $folder 'summary.txt'
tracerpt $etl -of CSV -o (Join-Path $folder 'events.csv') -summary $summary -y 2>&1 | Out-Null
if (Test-Path $summary) {
    Get-Content $summary | Select-String -Pattern 'Events processed|Events lost|Buffers lost|Microsoft-Windows-(Kernel-Network|Kernel-Process|DNS-Client)|^Total' | ForEach-Object { $_.Line }
} else {
    Write-Host 'tracerpt produced no summary (the session captured nothing).' -ForegroundColor Yellow
}
Write-Host ''
Write-Host 'Read this as: if the probe shows Kernel-Network / Kernel-Process events, Windows ETW is healthy and the problem'
Write-Host 'is in the SYNTRAC trace session. If it shows none, ETW itself is stuck and a reboot is the first thing to try.'
Write-Host "Probe files kept in: $folder"
