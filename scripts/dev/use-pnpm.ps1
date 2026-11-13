# Dot-source this to make sure `pnpm` (and the `node` it needs) can be found, even from a terminal whose PATH does
# not include them (for example a conda-activated shell). Compatible with Windows PowerShell 5.1.
#
#   . (Join-Path $PSScriptRoot 'use-pnpm.ps1')
function Add-ToPathIfContains {
    param([string]$Executable, [string[]]$Candidates)
    if (Get-Command $Executable -ErrorAction SilentlyContinue) { return }
    foreach ($dir in $Candidates) {
        if ($dir -and (Test-Path $dir) -and (Get-ChildItem -Path $dir -Filter "$Executable*" -ErrorAction SilentlyContinue)) {
            $env:Path = "$dir;$env:Path"
            return
        }
    }
}

Add-ToPathIfContains 'node' @('C:\nvm4w\nodejs', (Join-Path $env:ProgramFiles 'nodejs'))
Add-ToPathIfContains 'pnpm' @((Join-Path $env:LOCALAPPDATA 'pnpm\bin'), (Join-Path $env:LOCALAPPDATA 'pnpm'), $env:PNPM_HOME)

foreach ($tool in 'node', 'pnpm') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool was not found on PATH or in the usual install folders. Install it (pnpm: https://pnpm.io/installation) and reopen the terminal."
    }
}
