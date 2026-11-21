# Runs the REAL app on a FICTIONAL machine (docs/PUBLIC-PLAN.md): a demo database made from nothing, a stand-in collector that keeps
# it fresh and answers the app's status request, and the app itself. Nothing real is read or written: the app gets its own
# per-user folder and its own app identity (so a SYNTRAC you are running is not touched), and the database holds only invented data.
#
#   powershell -ExecutionPolicy Bypass -File scripts/dev/run-demo.ps1                     # try the app on the demo machine
#   powershell -ExecutionPolicy Bypass -File scripts/dev/run-demo.ps1 -Capture docs/images  # take the README screenshots, then stop
#
# Everything the script makes is in one new folder under %TEMP% (printed at the end); nothing is deleted.
param(
    [string]$Capture,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
Set-Location $root
. (Join-Path $PSScriptRoot 'use-cargo.ps1')
. (Join-Path $PSScriptRoot 'use-pnpm.ps1')

$pipe = 'syntrac-demo'
$debugPort = 9333
$work = Join-Path $env:TEMP ('syntrac-demo-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $work | Out-Null
$now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()

if (-not $SkipBuild) {
    Write-Host '==> building the screens' -ForegroundColor Cyan
    Push-Location (Join-Path $root 'app')
    pnpm build | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'pnpm build failed' }
    Pop-Location
    Write-Host '==> building the demo tools and the app (with its own demo identity)' -ForegroundColor Cyan
    cargo build -p syntrac-demo
    if ($LASTEXITCODE -ne 0) { throw 'cargo build (demo) failed' }
    $env:TAURI_CONFIG = '{"identifier":"dev.syntrac.app.demo"}'
    # custom-protocol embeds the screens in the program (a plain debug build looks for the development server instead).
    try { cargo build -p syntrac-app --features tauri/custom-protocol } finally { Remove-Item Env:\TAURI_CONFIG -ErrorAction SilentlyContinue }
    if ($LASTEXITCODE -ne 0) { throw 'cargo build (app) failed' }
}

Write-Host '==> making the demo database' -ForegroundColor Cyan
$database = Join-Path $work 'demo.db'
& (Join-Path $root 'target\debug\make-demo-db.exe') $database $now
if ($LASTEXITCODE -ne 0) { throw 'make-demo-db failed' }

Write-Host '==> starting the stand-in collector' -ForegroundColor Cyan
$collector = Start-Process -FilePath (Join-Path $root 'target\debug\demo-collector.exe') `
    -ArgumentList @("`"$database`"", "$now", $pipe) -PassThru -WindowStyle Hidden
$appCopy = Join-Path $work 'syntrac-demo-app.exe'
Copy-Item (Join-Path $root 'target\debug\syntrac-app.exe') $appCopy
$app = $null
try {
    Start-Sleep -Seconds 2
    $saved = @{ L = $env:LOCALAPPDATA; D = $env:SYNTRAC_DB; P = $env:SYNTRAC_PIPE; W = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS }
    $env:LOCALAPPDATA = Join-Path $work 'local'
    $env:SYNTRAC_DB = $database
    $env:SYNTRAC_PIPE = $pipe
    if ($Capture) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$debugPort" }
    Write-Host '==> starting the app' -ForegroundColor Cyan
    $app = Start-Process -FilePath $appCopy -PassThru
    $env:LOCALAPPDATA = $saved.L; $env:SYNTRAC_DB = $saved.D; $env:SYNTRAC_PIPE = $saved.P
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $saved.W

    if ($Capture) {
        . (Join-Path $PSScriptRoot 'demo-capture.ps1')
        $out = if ([IO.Path]::IsPathRooted($Capture)) { $Capture } else { Join-Path $root $Capture }
        New-Item -ItemType Directory -Force -Path $out | Out-Null
        Save-DemoScreenshots -Port $debugPort -OutputFolder $out
    } else {
        Write-Host ''
        Write-Host 'The demo is running. Close the window to hide it; Quit from the tray icon (or press Ctrl+C here) to stop everything.' -ForegroundColor Green
        Wait-Process -Id $app.Id
    }
} finally {
    if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force }
    if ($collector -and -not $collector.HasExited) { Stop-Process -Id $collector.Id -Force }
    Write-Host "Demo files are in $work (nothing was deleted)."
}
