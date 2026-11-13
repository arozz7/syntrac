# Builds the collector and runs it in console mode in an ELEVATED window (a UAC prompt will appear).
# Each run writes its own files, named with the same timestamp, and nothing is ever deleted:
#   aiChangeLog/console-run-<stamp>.txt   the run summary (includes the startup snapshot's skipped count)
#   data/syntrac-<stamp>.db               the database (git-ignored)
# Inspect a run afterwards with:  python -I scripts/dev/inspect-run.py data/syntrac-<stamp>.db
# Compatible with Windows PowerShell 5.1.
param(
    [int]$Seconds = 60
)

$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
Set-Location $root

. (Join-Path $PSScriptRoot 'use-cargo.ps1')
cargo build -p syntrac-service
if ($LASTEXITCODE -ne 0) { Write-Host 'Build failed' -ForegroundColor Red; exit 1 }

New-Item -ItemType Directory -Force -Path (Join-Path $root 'data') | Out-Null
$exe = Join-Path $root 'target/debug/syntrac-service.exe'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$db = Join-Path $root "data/syntrac-$stamp.db"
$result = Join-Path $root "aiChangeLog/console-run-$stamp.txt"
$log = Join-Path $root "data/console-run-$stamp.log"

$inner = "Set-Location '$root'; & '$exe' --seconds $Seconds --db '$db' --summary-file '$result' *>&1 | Tee-Object -FilePath '$log'; Write-Host ''; Read-Host 'Done. Press Enter to close'"

Write-Host "Launching the elevated collector for $Seconds seconds."
Write-Host 'While it runs: use the machine normally AND open/close a few programs (Notepad, Calculator),'
Write-Host 'browse to a site or two, and run something CPU-heavy for a few seconds if you can.'
Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $inner
Write-Host "Summary:  $result"
Write-Host "Database: $db"
Write-Host "Live log: $log"
Write-Host "Inspect:  python -I scripts/dev/inspect-run.py $db"


