# Small helpers shared by the deployment scripts that control the Windows service. Compatible with Windows PowerShell 5.1.

function Assert-Elevated {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'This script must run in an elevated (Administrator) PowerShell.'
    }
}

function Wait-ServiceState {
    param([Parameter(Mandatory = $true)][string]$Name, [Parameter(Mandatory = $true)][string]$State, [int]$Seconds = 60)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $service = Get-Service -Name $Name -ErrorAction SilentlyContinue
        if ($service -and $service.Status.ToString() -eq $State) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

# Runs the collector executable and returns its exit code and output lines. Windows PowerShell 5.1 turns ANY text a
# native program writes to stderr into a terminating error when $ErrorActionPreference is Stop, and the collector
# reports problems on stderr on purpose, so the preference is relaxed just for the call.
function Invoke-Collector {
    param([Parameter(Mandatory = $true)][string]$Executable, [string[]]$Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # stderr lines arrive as ErrorRecords; use their message so the output reads as plain text.
        $lines = @(& $Executable @Arguments 2>&1 | ForEach-Object {
            if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" }
        })
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [pscustomobject]@{ ExitCode = $code; Lines = [string[]]$lines }
}
