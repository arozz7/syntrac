# Pure helpers of the installer build (build-installer.ps1). Compatible with Windows PowerShell 5.1.
# They take text and names and return answers, so they can be tested without building anything.

# The version under [workspace.package] of the root Cargo.toml, or $null.
function Get-WorkspaceVersion {
    param([Parameter(Mandatory = $true)][string]$CargoToml)
    $inSection = $false
    foreach ($line in ($CargoToml -split "`r?`n")) {
        if ($line -match '^\s*\[(.+)\]\s*$') { $inSection = ($Matches[1] -eq 'workspace.package'); continue }
        if ($inSection -and $line -match '^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
    }
    return $null
}

# The top-level "version" of a JSON file (tauri.conf.json, package.json), or $null.
function Get-JsonVersion {
    param([Parameter(Mandatory = $true)][string]$Json)
    $value = (ConvertFrom-Json $Json).version
    if ($value) { return [string]$value }
    return $null
}

# One sentence for every way the given versions (name -> version) disagree or are unusable; none when all agree.
function Find-VersionMismatches {
    param([Parameter(Mandatory = $true)][hashtable]$Versions)
    $problems = @()
    foreach ($name in ($Versions.Keys | Sort-Object)) {
        $version = $Versions[$name]
        if (-not $version) { $problems += "$name has no version"; continue }
        if ($version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$') { $problems += "$name has the version '$version', which is not major.minor.patch" }
    }
    $present = @($Versions.Keys | Where-Object { $Versions[$_] } | Sort-Object)
    if ($present.Count -gt 1) {
        $reference = $Versions[$present[0]]
        foreach ($name in ($present | Select-Object -Skip 1)) {
            if ($Versions[$name] -ne $reference) { $problems += "$name is $($Versions[$name]) but $($present[0]) is $reference" }
        }
    }
    return $problems
}

# Of the file names in scripts\deploy, the ones the installer carries: what installing, upgrading, removing and moving the data need. Self-tests,
# development smoke runs and Python helpers stay in the repository.
function Select-ShippedDeployFiles {
    param([Parameter(Mandatory = $true)][string[]]$Names)
    return @($Names | Where-Object { $_ -like '*.ps1' -and $_ -notlike '*.tests.ps1' -and $_ -notlike 'smoke-*' })
}
