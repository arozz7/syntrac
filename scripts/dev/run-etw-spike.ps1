# Builds the ETW spike and runs it in an elevated window (a UAC prompt will appear).
# Output is saved to aiChangeLog/etw-spike-result-kw<mask>.txt so runs with different masks do not overwrite each other.
# Compatible with Windows PowerShell 5.1.
param(
    [int]$Seconds = 30,
    # Kernel-Process keyword mask in hex: 10 = process start/stop only, 0 = everything (unfiltered).
    [string]$ProcessKeywords = '10'
)

$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
Set-Location $root

cargo build -p syntrac-collector --bin etw_spike
if ($LASTEXITCODE -ne 0) { Write-Host 'Build failed' -ForegroundColor Red; exit 1 }

$exe = Join-Path $root 'target/debug/etw_spike.exe'
$result = Join-Path $root "aiChangeLog/etw-spike-result-kw$ProcessKeywords.txt"
$inner = "& '$exe' $Seconds '$ProcessKeywords' *>&1 | Tee-Object -FilePath '$result'; Write-Host ''; Read-Host 'Done. Press Enter to close'"

Write-Host "Launching elevated spike for $Seconds seconds (Kernel-Process keywords 0x$ProcessKeywords)."
Write-Host 'While it runs: use the machine normally and open/close a few programs (Notepad, Calculator).'
Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $inner
Write-Host "Results will be written to $result"
