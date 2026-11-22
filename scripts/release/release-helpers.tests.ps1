# Self-test for release-helpers.ps1 (run by verify.ps1). Exit code 0 = all passed.
. (Join-Path $PSScriptRoot 'release-helpers.ps1')

$script:failures = 0
function Assert-That {
    param([string]$Name, [bool]$Condition)
    if ($Condition) { Write-Host "  ok   $Name" } else { Write-Host "  FAIL $Name" -ForegroundColor Red; $script:failures++ }
}

# --- reading versions
$cargo = "[workspace]`r`nmembers = []`r`n`r`n[workspace.package]`r`nversion = `"1.2.3`"`r`nedition = `"2021`"`r`n`r`n[dependencies]`r`nversion = `"9.9.9`"`r`n"
Assert-That 'the workspace version is the one under [workspace.package]' ((Get-WorkspaceVersion $cargo) -eq '1.2.3')
Assert-That 'a Cargo.toml without a workspace version gives nothing' ($null -eq (Get-WorkspaceVersion "[package]`r`nversion = `"4.5.6`"`r`n"))
Assert-That 'a JSON version is read' ((Get-JsonVersion '{ "name": "x", "version": "0.1.0" }') -eq '0.1.0')
Assert-That 'a JSON without a version gives nothing' ($null -eq (Get-JsonVersion '{ "name": "x" }'))

# --- agreement
Assert-That 'versions that agree give no problems' (@(Find-VersionMismatches @{ cargo = '0.2.0'; tauri = '0.2.0'; package = '0.2.0' }).Count -eq 0)
$problems = @(Find-VersionMismatches @{ cargo = '0.2.0'; tauri = '0.1.0'; package = '0.2.0' })
Assert-That 'a differing version is reported' ($problems.Count -eq 1)
Assert-That 'the report names the file that differs' ($problems[0] -like '*tauri*0.1.0*')
Assert-That 'a missing version is reported' (@(Find-VersionMismatches @{ cargo = '0.2.0'; tauri = $null }).Count -ge 1)
Assert-That 'a version that is not major.minor.patch is reported' (@(Find-VersionMismatches @{ cargo = 'v0.2'; tauri = 'v0.2' }).Count -ge 1)
Assert-That 'a pre-release suffix is accepted' (@(Find-VersionMismatches @{ cargo = '0.2.0-rc.1'; tauri = '0.2.0-rc.1' }).Count -eq 0)

# --- which deploy scripts go into the installer
$all = @('install-service.ps1', 'uninstall-service.ps1', 'config-helpers.ps1', 'config-helpers.tests.ps1', 'data-root.ps1', 'data-root.tests.ps1',
         'service-common.ps1', 'move-data.ps1', 'smoke-elevated.ps1', 'smoke-test-service.ps1', 'check_db.py', 'installer-user.tests.ps1')
$shipped = @(Select-ShippedDeployFiles $all)
Assert-That 'the install and uninstall scripts ship' (($shipped -contains 'install-service.ps1') -and ($shipped -contains 'uninstall-service.ps1'))
Assert-That 'the helpers they load ship' (($shipped -contains 'config-helpers.ps1') -and ($shipped -contains 'data-root.ps1') -and ($shipped -contains 'service-common.ps1'))
Assert-That 'move-data ships, so the data can be moved later' ($shipped -contains 'move-data.ps1')
Assert-That 'no self-test ships' (@($shipped | Where-Object { $_ -like '*.tests.ps1' }).Count -eq 0)
Assert-That 'no development smoke script ships' (@($shipped | Where-Object { $_ -like 'smoke-*' }).Count -eq 0)
Assert-That 'a Python helper does not ship' (-not ($shipped -contains 'check_db.py'))

if ($script:failures -gt 0) { Write-Host "$($script:failures) failed" -ForegroundColor Red; exit 1 }
Write-Host 'release-helpers: all passed'
exit 0
