# Runs INSIDE Windows Sandbox (run-installer-test.ps1 -Pages): opens the installer and the uninstaller as a person would and takes a picture of every page,
# pressing Enter (the default button) to move on. Pictures and report.txt go to C:\Results. Compatible with Windows PowerShell 5.1.
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$installer = (Get-ChildItem 'C:\SetupTest\good\*_x64-setup.exe' | Select-Object -First 1).FullName
$installDir = 'C:\Program Files\SYNTRAC'
$shots = 'C:\Results\pages'
New-Item -ItemType Directory -Force -Path $shots | Out-Null
$report = 'C:\Results\report.txt'
$shell = New-Object -ComObject WScript.Shell
$script:number = 0

function Say { param([string]$Text) Add-Content -Path $report -Value $Text; Write-Host $Text }
function Shot {
    param([string]$Label)
    $script:number++
    $bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $name = '{0:00}-{1}.png' -f $script:number, $Label
    $bitmap.Save((Join-Path $shots $name), [Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    Say "picture $name"
}
# Pictures of each page, then Enter on the window that is in front, until the program has ended or the pages run out.
function Walk {
    param([string]$Name, [string]$Path, [string]$Arguments = '', [int]$MaxPages = 9)
    Say "== $Name"
    if ($Arguments) { $process = Start-Process -FilePath $Path -ArgumentList $Arguments -PassThru } else { $process = Start-Process -FilePath $Path -PassThru }
    Start-Sleep -Seconds 6
    for ($page = 1; $page -le $MaxPages; $page++) {
        Shot "$Name-page$page"
        [void]$shell.AppActivate($process.Id)
        Start-Sleep -Milliseconds 400
        [Windows.Forms.SendKeys]::SendWait('{ENTER}')
        Start-Sleep -Seconds 5
        if ($process.HasExited) { Say "$Name ended after page $page (exit code $($process.ExitCode))"; break }
    }
    Start-Sleep -Seconds 3
    Shot "$Name-after"
}
function State { $s = Get-Service -Name Syntrac -ErrorAction SilentlyContinue; if ($s) { return $s.Status.ToString() } return 'missing' }

Set-Content -Path $report -Value "SYNTRAC installer pages, $(Get-Date -Format s)"
Walk 'install' $installer
Say "service after the install: $(State)"
Say "app installed: $(Test-Path (Join-Path $installDir 'syntrac-app.exe'))"
Get-Process -Name syntrac-app -ErrorAction SilentlyContinue | ForEach-Object { Say "the app is running (window title '$($_.MainWindowTitle)')" }
Shot 'desktop-after-install'

# The installer again, over the installed one: the page that offers to reinstall or uninstall first.
Get-Process -Name syntrac-app -ErrorAction SilentlyContinue | Stop-Process -Force
Walk 'second-install' $installer
Say "service after the second install: $(State)"

# The uninstaller as the Settings page would start it.
Get-Process -Name syntrac-app -ErrorAction SilentlyContinue | Stop-Process -Force
Walk 'uninstall' (Join-Path $installDir 'uninstall.exe')
$deadline = (Get-Date).AddSeconds(90)
while ((State) -ne 'missing' -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 2 }
Say "service after the uninstall: $(State)"
Say "config kept: $(Test-Path (Join-Path $installDir 'config\service.toml'))"
Say "data kept: $(Test-Path 'C:\ProgramData\Syntrac\data\syntrac.db')"
Say 'DONE'
