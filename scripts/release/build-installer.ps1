# Builds the Windows installer (NSIS, per machine, UNSIGNED unless signing is asked for). Compatible with Windows PowerShell 5.1.
#
#   powershell -ExecutionPolicy Bypass -File scripts\release\build-installer.ps1
#
# 1. Checks the version is the same in Cargo.toml, tauri.conf.json and package.json.
# 2. Builds the service in release mode and stages the payload (the service, its config template and the deploy scripts) in
#    app\src-tauri\installer-stage\setup\, laid out like the repository so the deploy scripts run from it unchanged.
# 3. Runs `tauri build` with tauri.installer.conf.json, which wraps the app and the payload in an NSIS installer.
# 4. Prints the installer's path and SHA-256.
#
# Nothing here installs anything: run the installer only in a throw-away machine (see docs/RELEASE.md). The first build downloads the NSIS tools
# once.
param(
    # Stage the payload and stop: no app build, no installer. For checking what would be shipped.
    [switch]$StageOnly,
    # Sign the installer with this certificate (its thumbprint in the Windows certificate store). OFF by default; see docs/RELEASE.md.
    [string]$CertificateThumbprint,
    [string]$TimestampUrl = 'http://timestamp.digicert.com',
    # FOR THE INSTALLER TEST ONLY: ship this program as the service instead of the real one (the test uses a stand-in that cannot start, to prove an
    # upgrade rolls back). Never use it for a release.
    [string]$ServiceExe,
    # Also copy the finished installer into this folder (it is created if needed).
    [string]$CopyInstallerTo
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
# The installer is built in its own target folder: a program that is running from target/release (the app in use, a console run) cannot be
# overwritten, and a release for others should not depend on whatever the development builds left behind. The first build compiles everything.
$targetDir = Join-Path $root 'target/installer'
$env:CARGO_TARGET_DIR = $targetDir
. (Join-Path $PSScriptRoot 'release-helpers.ps1')
. (Join-Path $PSScriptRoot '../dev/use-cargo.ps1')
. (Join-Path $PSScriptRoot '../dev/use-pnpm.ps1')

function Invoke-Checked {
    param([string]$Name, [scriptblock]$Command)
    Write-Host "==> $Name" -ForegroundColor Cyan
    $global:LASTEXITCODE = 0
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed (exit code $LASTEXITCODE)" }
}

# 1. One version everywhere.
$tauriConfigPath = Join-Path $root 'app/src-tauri/tauri.conf.json'
$versions = @{
    'Cargo.toml'      = Get-WorkspaceVersion (Get-Content -Raw (Join-Path $root 'Cargo.toml'))
    'tauri.conf.json' = Get-JsonVersion (Get-Content -Raw $tauriConfigPath)
    'package.json'    = Get-JsonVersion (Get-Content -Raw (Join-Path $root 'app/package.json'))
}
$problems = @(Find-VersionMismatches $versions)
if ($problems.Count -gt 0) { throw "The version is not the same everywhere: $($problems -join '; ')" }
$version = $versions['Cargo.toml']
Write-Host "Version $version"

# 2. The payload. The folder is ours (generated, ignored by git) and only ever emptied after checking it is exactly that folder.
if ($ServiceExe) {
    if (-not (Test-Path $ServiceExe)) { throw "-ServiceExe not found: $ServiceExe" }
    Write-Host "WARNING: shipping $ServiceExe as the service. This installer is for the installer test only, not for a release." -ForegroundColor Yellow
    $serviceSource = (Resolve-Path $ServiceExe).Path
} else {
    Invoke-Checked 'cargo build --release -p syntrac-service' { cargo build --release -p syntrac-service }
    $serviceSource = Join-Path $targetDir 'release/syntrac-service.exe'
}
$stage = Join-Path $root 'app/src-tauri/installer-stage'
$setup = Join-Path $stage 'setup'
if ((Split-Path -Leaf $stage) -ne 'installer-stage' -or -not $stage.StartsWith($root)) { throw "Unexpected stage folder: $stage" }
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path (Join-Path $setup 'config'), (Join-Path $setup 'scripts/deploy') | Out-Null
Copy-Item $serviceSource (Join-Path $setup 'syntrac-service.exe')
Copy-Item (Join-Path $root 'config/service.toml') (Join-Path $setup 'config/service.toml')
$deployNames = Get-ChildItem (Join-Path $root 'scripts/deploy') -File | ForEach-Object { $_.Name }
foreach ($name in (Select-ShippedDeployFiles $deployNames)) {
    Copy-Item (Join-Path $root "scripts/deploy/$name") (Join-Path $setup "scripts/deploy/$name")
}
Write-Host 'Payload:'
Get-ChildItem $setup -Recurse -File | ForEach-Object { Write-Host ('  ' + $_.FullName.Substring($setup.Length + 1)) }
if ($StageOnly) { Write-Host "Staged in $setup (nothing built)."; return }

# 3. The installer.
$appDir = Join-Path $root 'app'
Push-Location $appDir
try {
    Invoke-Checked 'pnpm install' { pnpm install --frozen-lockfile }
    Invoke-Checked 'tauri build (NSIS)' { pnpm tauri build --config (Join-Path $root 'app/src-tauri/tauri.installer.conf.json') }
} finally {
    Pop-Location
}

# 4. The result.
$installer = Get-ChildItem (Join-Path $targetDir 'release/bundle/nsis') -Filter "*_${version}_x64-setup.exe" |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) { throw "No installer for version $version was found in target\release\bundle\nsis." }

if ($CertificateThumbprint) {
    $signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if (-not $signtool) { throw 'signtool.exe was not found (install the Windows SDK) but a certificate was given.' }
    Invoke-Checked 'signtool sign' { & $signtool.Source sign /sha1 $CertificateThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $installer.FullName }
    $signed = 'signed'
} else {
    $signed = 'UNSIGNED (Windows SmartScreen will warn when it is run; see docs/RELEASE.md)'
}

if ($CopyInstallerTo) {
    New-Item -ItemType Directory -Force -Path $CopyInstallerTo | Out-Null
    Copy-Item $installer.FullName (Join-Path $CopyInstallerTo $installer.Name) -Force
    Write-Host "Copied to $CopyInstallerTo"
}

$hash = (Get-FileHash -Algorithm SHA256 $installer.FullName).Hash.ToLowerInvariant()
Write-Host ''
Write-Host "Installer: $($installer.FullName)"
Write-Host "Size:      $([math]::Round($installer.Length / 1MB, 1)) MB"
Write-Host "SHA-256:   $hash"
Write-Host "State:     $signed"
