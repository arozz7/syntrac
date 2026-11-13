# Enforces source file size limits: soft limit warns, hard limit fails.
# Compatible with Windows PowerShell 5.1.
param(
    [int]$Soft = 600,
    [int]$Hard = 900
)

$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
$extensions = '.rs', '.ts', '.tsx', '.css', '.ps1'
$excluded = '\\(node_modules|target|dist|gen|\.git)\\'

$files = Get-ChildItem -Path $root -Recurse -File |
    Where-Object { $extensions -contains $_.Extension -and $_.FullName -notmatch $excluded }

$overHard = 0
foreach ($file in $files) {
    $lines = (Get-Content -LiteralPath $file.FullName | Measure-Object -Line).Lines
    $relative = $file.FullName.Substring($root.Path.Length + 1)
    if ($lines -gt $Hard) {
        Write-Host "FAIL  $relative has $lines lines (hard limit $Hard) - split using the refactoring protocol" -ForegroundColor Red
        $overHard++
    }
    elseif ($lines -gt $Soft) {
        Write-Host "WARN  $relative has $lines lines (soft limit $Soft) - consider splitting" -ForegroundColor Yellow
    }
}

if ($overHard -gt 0) { exit 1 }
Write-Host "File size check passed ($($files.Count) files)"
exit 0
