# Tries the installer in Windows Sandbox, a throw-away copy of Windows, so nothing on this machine is installed, replaced or removed.
# Compatible with Windows PowerShell 5.1.
#
#   powershell -ExecutionPolicy Bypass -File scripts\release\sandbox\run-installer-test.ps1
#
# Needs Windows Sandbox (Windows 11 Pro/Enterprise): once, in an elevated PowerShell,
#   Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All
# and restart. Builds two installers (the real one, and one whose service cannot start, to prove the rollback), starts the sandbox, and waits for the
# report that sandbox-test.ps1 writes. The sandbox has no network and is gone when its window closes.
param(
    # Use the installers already built under target\installer-test instead of building them again.
    [switch]$SkipBuild,
    # Instead of the checks, click through the installer's and the uninstaller's pages and take a picture of each (results\pages).
    [switch]$Pages,
    [int]$TimeoutMinutes = 20
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$work = Join-Path $root 'target/installer-test'
$goodDir = Join-Path $work 'good'
$brokenDir = Join-Path $work 'broken'
$resultsDir = Join-Path $work 'results'
$sandboxExe = Join-Path $env:SystemRoot 'System32\WindowsSandbox.exe'

if (-not (Test-Path $sandboxExe)) {
    Write-Host 'Windows Sandbox is not installed on this machine.' -ForegroundColor Yellow
    Write-Host 'In an elevated PowerShell run:  Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All'
    Write-Host 'then restart Windows, and run this script again.'
    exit 2
}

# Windows allows one sandbox at a time: a second launch quietly joins the first, and the test then never starts.
if (Get-Process -Name WindowsSandboxServer, vmmemWindowsSandbox -ErrorAction SilentlyContinue) {
    Write-Host 'A Windows Sandbox is already running. Close its window and run this again.' -ForegroundColor Yellow
    exit 3
}

if (-not $SkipBuild) {
    # Installers of earlier versions in these folders (made by this script, under target\) would be picked up by the test instead of the new ones.
    foreach ($dir in $goodDir, $brokenDir) {
        if (Test-Path $dir) { Get-ChildItem (Join-Path $dir '*_x64-setup*.exe') | Remove-Item -Force }
    }
    $build = Join-Path $root 'scripts/release/build-installer.ps1'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $build -CopyInstallerTo $goodDir
    if ($LASTEXITCODE -ne 0) { throw 'The installer did not build.' }

    . (Join-Path $root 'scripts/dev/use-cargo.ps1')
    $env:CARGO_TARGET_DIR = Join-Path $root 'target/installer'
    # Like the real build: a clean Windows has no Visual C++ runtime, so the stand-in carries it too.
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    cargo build --release -p syntrac-demo --bin broken-service
    if ($LASTEXITCODE -ne 0) { throw 'The stand-in service did not build.' }
    & powershell -NoProfile -ExecutionPolicy Bypass -File $build -ServiceExe (Join-Path $root 'target/installer/release/broken-service.exe') -CopyInstallerTo $brokenDir
    if ($LASTEXITCODE -ne 0) { throw 'The failing installer did not build.' }
    # The normal installer is built last so the staged payload on disk is the real one again.
    & powershell -NoProfile -ExecutionPolicy Bypass -File $build -StageOnly
}

foreach ($dir in $goodDir, $brokenDir) {
    if (-not (Get-ChildItem (Join-Path $dir '*_x64-setup*.exe') -ErrorAction SilentlyContinue)) { throw "No installer in $dir (run without -SkipBuild)." }
}
New-Item -ItemType Directory -Force -Path $resultsDir | Out-Null
$reportFile = Join-Path $resultsDir 'report.txt'
if (Test-Path $reportFile) { Remove-Item $reportFile -Force }
if ($Pages -and (Test-Path (Join-Path $resultsDir 'pages'))) { Remove-Item (Join-Path $resultsDir 'pages') -Recurse -Force }

$guestName = 'sandbox-test.ps1'
if ($Pages) { $guestName = 'pages-test.ps1' }
$configFile = Join-Path $work 'installer-test.wsb'
$xml = @"
<Configuration>
  <Networking>Disable</Networking>
  <MappedFolders>
    <MappedFolder><HostFolder>$goodDir</HostFolder><SandboxFolder>C:\SetupTest\good</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>$brokenDir</HostFolder><SandboxFolder>C:\SetupTest\broken</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>$PSScriptRoot</HostFolder><SandboxFolder>C:\SetupTest\scripts</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>$resultsDir</HostFolder><SandboxFolder>C:\Results</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>
  </MappedFolders>
  <LogonCommand><Command>powershell -NoProfile -ExecutionPolicy Bypass -File C:\SetupTest\scripts\$guestName</Command></LogonCommand>
</Configuration>
"@
Set-Content -Path $configFile -Value $xml -Encoding UTF8
Write-Host "Starting Windows Sandbox ($configFile)..."
Start-Process -FilePath $sandboxExe -ArgumentList "`"$configFile`""

$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
while ((Get-Date) -lt $deadline) {
    if ((Test-Path $reportFile) -and ((Get-Content $reportFile -Raw) -match '(?m)^DONE\s*$')) { break }
    Start-Sleep -Seconds 5
}
if (-not (Test-Path $reportFile)) { throw "No report appeared within $TimeoutMinutes minutes: $reportFile" }
Get-Content $reportFile
if ($Pages) { Write-Host "Pictures are in $resultsDir\pages (close the sandbox window)"; exit 0 }
if ((Get-Content $reportFile -Raw) -match 'ALL CHECKS PASSED') { Write-Host 'Installer test: PASSED (close the sandbox window)' -ForegroundColor Green; exit 0 }
Write-Host 'Installer test: FAILED' -ForegroundColor Red
exit 1
