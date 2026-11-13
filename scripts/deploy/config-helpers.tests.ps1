# Self-test for config-helpers.ps1 (run by verify.ps1). Exit code 0 = all passed.
. (Join-Path $PSScriptRoot 'config-helpers.ps1')

$script:failures = 0
function Assert-That {
    param([string]$Name, [bool]$Condition)
    if ($Condition) { Write-Host "  ok   $Name" } else { Write-Host "  FAIL $Name" -ForegroundColor Red; $script:failures++ }
}

$expected = Get-ExpectedConfigPaths -DataRoot 'C:\ProgramData\Syntrac'
Assert-That 'database lives in data\' ($expected.Database -eq 'C:/ProgramData/Syntrac/data/syntrac.db')
Assert-That 'logs are a SIBLING of data\, not inside it (the original bug)' ($expected.Logs -eq 'C:/ProgramData/Syntrac/logs')
Assert-That 'logs path is not under the user-writable data folder' (-not $expected.Logs.StartsWith('C:/ProgramData/Syntrac/data'))

$template = "flow_window_secs = 10`r`n"
$generated = New-ConfigText -Template $template -Database $expected.Database -Logs $expected.Logs
Assert-That 'a generated config matches its own expected paths' (Test-ConfigPaths $generated $expected.Database $expected.Logs)
Assert-That 'the template is carried over' ($generated.Contains('flow_window_secs = 10'))

$buggy = "database_path = `"C:/ProgramData/Syntrac/data/syntrac.db`"`r`nlog_dir = `"C:/ProgramData/Syntrac/data/logs`"`r`n"
Assert-That 'the config written by the buggy installer is detected as wrong' (-not (Test-ConfigPaths $buggy $expected.Database $expected.Logs))

Assert-That 'a config without the keys is detected as wrong' (-not (Test-ConfigPaths 'flow_window_secs = 10' $expected.Database $expected.Logs))
Assert-That 'a commented-out key does not count' (-not (Test-ConfigPaths "# database_path = `"$($expected.Database)`"`r`n# log_dir = `"$($expected.Logs)`"" $expected.Database $expected.Logs))
Assert-That 'a custom data root is honoured' ((Get-ExpectedConfigPaths -DataRoot 'D:\Telemetry').Logs -eq 'D:/Telemetry/logs')

if ($script:failures -gt 0) { Write-Host "$($script:failures) failed" -ForegroundColor Red; exit 1 }
Write-Host 'config helpers: all passed'
exit 0