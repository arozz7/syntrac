# Dot-source this to make sure `cargo` can be found, even from a terminal whose PATH does not include the Rust
# toolchain (for example a conda-activated shell). Compatible with Windows PowerShell 5.1.
#
#   . (Join-Path $PSScriptRoot 'use-cargo.ps1')
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:Path = "$cargoBin;$env:Path"
    } else {
        throw "cargo was not found on PATH or in $cargoBin. Install Rust (https://rustup.rs) and reopen the terminal."
    }
}