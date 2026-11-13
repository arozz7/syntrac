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

function Invoke-Step {
    param([string]$Name, [scriptblock]$Command)
    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $Name (exit code $LASTEXITCODE)" -ForegroundColor Red
        exit 1
    }
}

$appDir = Join-Path $root 'app'
if ((Test-Path (Join-Path $appDir 'package.json')) -and -not $SkipFrontend) {
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

Invoke-Step 'file size check' { & (Join-Path $PSScriptRoot 'check-file-size.ps1') }

Write-Host ""
Write-Host "verify: ALL STEPS PASSED" -ForegroundColor Green
exit 0
