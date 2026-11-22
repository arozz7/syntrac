# Local verification gate. Runs format, lint (warnings are errors), build, tests and the
# file-size check, and stops at the first failure. Run before every push.
# Compatible with Windows PowerShell 5.1 and PowerShell 7+.
#
# The frontend runs first because the Tauri crate embeds app/dist at compile time.
param(
    [switch]$SkipFrontend
)

$ErrorActionPreference = 'Continue'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
Set-Location $root

. (Join-Path $PSScriptRoot 'use-cargo.ps1')
function Invoke-Step {
    param([string]$Name, [scriptblock]$Command)
    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    # A command that cannot even be found does not set $LASTEXITCODE, so a stale 0 from an earlier step would
    # look like success (and $? only describes the call of the script block, not what ran inside it).
    # Reset the exit code, and look for a "command not found" error record explicitly.
    $global:LASTEXITCODE = 0
    $Error.Clear()
    & $Command
    $notFound = @($Error | Where-Object { $_.FullyQualifiedErrorId -like 'CommandNotFoundException*' })
    if ($notFound.Count -gt 0) {
        Write-Host "FAILED: $Name (command not found: $($notFound[0].Exception.Message))" -ForegroundColor Red
        exit 1
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $Name (exit code $LASTEXITCODE)" -ForegroundColor Red
        exit 1
    }
}

$appDir = Join-Path $root 'app'
if ((Test-Path (Join-Path $appDir 'package.json')) -and -not $SkipFrontend) {
    . (Join-Path $PSScriptRoot 'use-pnpm.ps1')
    Push-Location $appDir
    Invoke-Step 'frontend: install (frozen lockfile)' { pnpm install --frozen-lockfile }
    Invoke-Step 'frontend: format check' { pnpm run format:check }
    Invoke-Step 'frontend: lint (warnings are errors)' { pnpm run lint }
    Invoke-Step 'frontend: typecheck' { pnpm run typecheck }
    Invoke-Step 'frontend: test' { pnpm run test }
    Invoke-Step 'frontend: build' { pnpm run build }
    Pop-Location
}

Invoke-Step 'rust: format check' { cargo fmt --all -- --check }
Invoke-Step 'rust: clippy (warnings are errors)' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Step 'rust: build' { cargo build --workspace }
Invoke-Step 'rust: test' { cargo test --workspace }

Invoke-Step 'deploy scripts: config helper self-test' { & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot '../deploy/config-helpers.tests.ps1') }

Invoke-Step 'deploy scripts: data root self-test' { & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot '../deploy/data-root.tests.ps1') }

Invoke-Step 'deploy scripts: installer decisions self-test' { & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot '../deploy/installer-helpers.tests.ps1') }

Invoke-Step 'release scripts: installer build helpers self-test' { & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot '../release/release-helpers.tests.ps1') }

Invoke-Step 'file size check' { & (Join-Path $PSScriptRoot 'check-file-size.ps1') }

Write-Host ""
Write-Host "verify: ALL STEPS PASSED" -ForegroundColor Green
exit 0
