# Read-only probe for slice I4b (docs/RUNAS-PLAN.md, "Step 0"). Run it in an ELEVATED PowerShell so it can read the Task
# Scheduler folder the way the SYSTEM service will. It writes nothing and changes nothing, and it prints COUNTS and SHAPES only:
# no task names, no program paths, no user names, no command lines. The output is safe to paste back.
#
#   powershell -ExecutionPolicy Bypass -File scripts\dev\probe-launchers.ps1

$ErrorActionPreference = 'Continue'

function Section($title) { Write-Output ''; Write-Output "== $title" }

function Count-By($items, [scriptblock]$key) {
    $items | Group-Object -Property { & $key $_ } | Sort-Object Count -Descending |
        ForEach-Object { '  {0,6}  {1}' -f $_.Count, $_.Name }
}

$elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
Write-Output "elevated: $elevated"
if (-not $elevated) { Write-Output 'NOTE: not elevated, so the Task Scheduler folder will probably show 0 files.' }

# ---- scheduled task definition files
Section 'Scheduled tasks (System32\Tasks)'
$root = Join-Path $env:SystemRoot 'System32\Tasks'
$files = @(Get-ChildItem -LiteralPath $root -Recurse -File -ErrorAction SilentlyContinue)
Write-Output ('files: {0}, largest: {1} KB' -f $files.Count, [math]::Round((($files | Measure-Object Length -Maximum).Maximum) / 1KB, 1))

$encodings = @{}
$parsed = 0
$failed = 0
$actionKinds = @{}
$triggerKinds = @{}
$execShapes = @{}
$disabled = 0
foreach ($file in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
    $bom = if ($bytes.Length -ge 2 -and $bytes[0] -eq 0xFF -and $bytes[1] -eq 0xFE) { 'UTF-16 LE with BOM' }
           elseif ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) { 'UTF-8 with BOM' }
           else { 'no BOM' }
    $encodings[$bom] = 1 + [int]$encodings[$bom]
    try {
        $xml = New-Object System.Xml.XmlDocument
        $xml.Load($file.FullName)
        $parsed++
        $ns = New-Object System.Xml.XmlNamespaceManager($xml.NameTable)
        $ns.AddNamespace('t', 'http://schemas.microsoft.com/windows/2004/02/mit/task')
        foreach ($action in $xml.SelectNodes('//t:Actions/*', $ns)) { $actionKinds[$action.LocalName] = 1 + [int]$actionKinds[$action.LocalName] }
        foreach ($trigger in $xml.SelectNodes('//t:Triggers/*', $ns)) { $triggerKinds[$trigger.LocalName] = 1 + [int]$triggerKinds[$trigger.LocalName] }
        if ($xml.SelectSingleNode('//t:Settings/t:Enabled', $ns).InnerText -eq 'false') { $disabled++ }
        foreach ($exec in $xml.SelectNodes('//t:Actions/t:Exec', $ns)) {
            $command = [string]$exec.SelectSingleNode('t:Command', $ns).InnerText
            $arguments = [string]$exec.SelectSingleNode('t:Arguments', $ns).InnerText
            $shape = @()
            if ($command.StartsWith('"')) { $shape += 'quoted' } else { $shape += 'unquoted' }
            if ($command -match '%[A-Za-z_]+%') { $shape += 'env-var' }
            if ($command -match '\s' -and -not $command.StartsWith('"')) { $shape += 'unquoted-with-space' }
            if ($arguments.Length -gt 0) { $shape += 'has-args' }
            if ($command -match '(?i)(^|[\\"])(rundll32|cmd|powershell|pwsh|wscript|cscript|mshta|msiexec|regsvr32)(\.exe)?"?\s*$') { $shape += 'proxy' }
            $label = $shape -join ','
            $execShapes[$label] = 1 + [int]$execShapes[$label]
        }
    } catch { $failed++ }
}
Write-Output "xml parsed: $parsed, failed: $failed, disabled tasks: $disabled"
Write-Output 'encodings:';       $encodings.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object { '  {0,6}  {1}' -f $_.Value, $_.Key }
Write-Output 'action kinds:';    $actionKinds.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object { '  {0,6}  {1}' -f $_.Value, $_.Key }
Write-Output 'trigger kinds:';   $triggerKinds.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object { '  {0,6}  {1}' -f $_.Value, $_.Key }
Write-Output 'Exec command shapes:'; $execShapes.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object { '  {0,6}  {1}' -f $_.Value, $_.Key }

# ---- Run and RunOnce keys: the shape of each value, not its text
Section 'Run and RunOnce values (shapes only)'
$keys = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run',
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce'
)
foreach ($sid in (Get-ChildItem Registry::HKEY_USERS -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match '^S-1-5-21-[\d-]+$' })) {
    $keys += "Registry::HKEY_USERS\$($sid.PSChildName)\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
    $keys += "Registry::HKEY_USERS\$($sid.PSChildName)\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce"
}
$runShapes = @{}
foreach ($key in $keys) {
    $item = Get-Item -LiteralPath $key -ErrorAction SilentlyContinue
    if ($null -eq $item) { continue }
    foreach ($name in $item.GetValueNames()) {
        $kind = $item.GetValueKind($name)
        $value = [string]$item.GetValue($name, $null, 'DoNotExpandEnvironmentNames')
        $shape = @("$kind")
        if ($value.StartsWith('"')) { $shape += 'quoted' } else { $shape += 'unquoted' }
        if ($value -match '%[A-Za-z_]+%') { $shape += 'env-var' }
        if ($value -match '\s' -and -not $value.StartsWith('"')) { $shape += 'unquoted-with-space' }
        if ($value -match '"\s+\S' -or ($value -notmatch '^"' -and $value -match '\.exe\s+\S')) { $shape += 'has-args' }
        if ($value -match '(?i)rundll32|cmd(\.exe)?\s|powershell|wscript|cscript|mshta|msiexec') { $shape += 'proxy' }
        $label = $shape -join ','
        $runShapes[$label] = 1 + [int]$runShapes[$label]
    }
}
$runShapes.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object { '  {0,6}  {1}' -f $_.Value, $_.Key }
Write-Output ('loaded user hives seen: {0}' -f @(Get-ChildItem Registry::HKEY_USERS -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match '^S-1-5-21-[\d-]+$' }).Count)

# ---- Startup folders
Section 'Startup folders'
$startup = @("$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Startup")
$startup += Get-ChildItem 'C:\Users' -Directory -ErrorAction SilentlyContinue |
    ForEach-Object { Join-Path $_.FullName 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup' }
foreach ($folder in $startup) {
    $entries = @(Get-ChildItem -LiteralPath $folder -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -ne 'desktop.ini' })
    if ($folder -like "$env:ProgramData*") { $label = 'all users' } else { $label = 'a user profile' }
    Write-Output ('  {0}: {1} entries; extensions: {2}' -f $label, $entries.Count, (($entries | Group-Object Extension | ForEach-Object { '{0} x{1}' -f $_.Name, $_.Count }) -join ', '))
}

# ---- services
Section 'Services (start types and command-line shapes)'
$services = @(Get-CimInstance Win32_Service)
Count-By $services { param($s) $s.StartMode }
$svcShapes = @{}
foreach ($service in $services) {
    $path = [string]$service.PathName
    $shape = @()
    if ($path.StartsWith('"')) { $shape += 'quoted' } else { $shape += 'unquoted' }
    if ($path -match '(?i)svchost\.exe') { $shape += 'svchost' }
    if ($path -match '\s' -and -not $path.StartsWith('"')) { $shape += 'unquoted-with-space' }
    if ($path -match '%[A-Za-z_]+%') { $shape += 'env-var' }
    if ($path -match '(?i)\\drivers\\|^\\SystemRoot') { $shape += 'driver-style' }
    $label = $shape -join ','
    $svcShapes[$label] = 1 + [int]$svcShapes[$label]
}
$svcShapes.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object { '  {0,6}  {1}' -f $_.Value, $_.Key }

Write-Output ''
Write-Output 'done: nothing was written or changed.'
