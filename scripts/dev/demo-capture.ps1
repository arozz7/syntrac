# Takes the README screenshots of the demo app (docs/PUBLIC-PLAN.md, P2). Dot-sourced by run-demo.ps1.
#
# It talks to the app's web view through its debugging port, which exists ONLY when run-demo.ps1 starts the demo app for this: it
# clicks through the screens and asks the web view for a picture of the PAGE. That is the page's own pixels, never a picture of the
# desktop, so nothing outside the app can end up in an image. The data on the page is the fictional machine of the demo database.

function Connect-DemoPage([int]$Port) {
    $deadline = (Get-Date).AddSeconds(90)
    $page = $null
    while (-not $page -and (Get-Date) -lt $deadline) {
        try {
            $targets = Invoke-RestMethod "http://127.0.0.1:$Port/json" -TimeoutSec 2
            $page = $targets | Where-Object { $_.type -eq 'page' -and $_.webSocketDebuggerUrl } | Select-Object -First 1
        } catch { Start-Sleep -Seconds 1 }
        if (-not $page) { Start-Sleep -Milliseconds 500 }
    }
    if (-not $page) { throw 'The demo app did not open its debugging port.' }
    $socket = New-Object System.Net.WebSockets.ClientWebSocket
    $socket.ConnectAsync([Uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
    return $socket
}

$script:cdpId = 0

# Sends one command and returns the raw reply text (the picture is too big for ConvertFrom-Json on Windows PowerShell 5.1).
function Invoke-Cdp($Socket, [string]$Method, $Params = @{}) {
    $script:cdpId++
    $id = $script:cdpId
    $message = @{ id = $id; method = $Method; params = $Params } | ConvertTo-Json -Depth 8 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($message)
    $Socket.SendAsync([ArraySegment[byte]]$bytes, [Net.WebSockets.WebSocketMessageType]::Text, $true, [Threading.CancellationToken]::None).Wait()
    while ($true) {
        $stream = New-Object System.IO.MemoryStream
        $buffer = New-Object byte[] 65536
        do {
            $result = $Socket.ReceiveAsync([ArraySegment[byte]]$buffer, [Threading.CancellationToken]::None).Result
            $stream.Write($buffer, 0, $result.Count)
        } until ($result.EndOfMessage)
        $text = [Text.Encoding]::UTF8.GetString($stream.ToArray())
        if ($text -match "^\{""id"":$id[,}]") { return $text }
    }
}

function Invoke-PageScript($Socket, [string]$Expression) {
    $reply = Invoke-Cdp $Socket 'Runtime.evaluate' @{ expression = $Expression; returnByValue = $true }
    $match = [regex]::Match($reply, '"value":"([^"]*)"')
    if ($match.Success) { return $match.Groups[1].Value }
    return ''
}

# Clicks the first button whose text starts with $Label; stops with an error when there is none (a renamed button must not be skipped).
function Click-DemoButton($Socket, [string]$Label, [switch]$Exact) {
    $test = if ($Exact) { "text === '$Label'" } else { "text.startsWith('$Label')" }
    $js = "(() => { const b = [...document.querySelectorAll('button')].find(x => { const text = x.textContent.trim().replace(/\s+/g, ' '); return $test; }); if (!b) return 'missing'; b.click(); return 'ok'; })()"
    if ((Invoke-PageScript $Socket $js) -ne 'ok') { throw "The button '$Label' is not on the page." }
    Start-Sleep -Milliseconds 2500
}

function Save-DemoShot($Socket, [string]$Folder, [string]$Name, [int]$Height = 900, [int]$Width = 1280) {
    Invoke-Cdp $Socket 'Emulation.setDeviceMetricsOverride' @{ width = $Width; height = $Height; deviceScaleFactor = 1.5; mobile = $false } | Out-Null
    Start-Sleep -Milliseconds 1200
    $reply = Invoke-Cdp $Socket 'Page.captureScreenshot' @{ format = 'png' }
    $match = [regex]::Match($reply, '"data":"([^"]+)"')
    if (-not $match.Success) { throw "No picture came back for $Name." }
    $path = Join-Path $Folder "$Name.png"
    [IO.File]::WriteAllBytes($path, [Convert]::FromBase64String($match.Groups[1].Value))
    Write-Host ("  {0}  ({1:N0} KB)" -f $Name, ((Get-Item $path).Length / 1KB))
}

function Save-DemoScreenshots([int]$Port, [string]$OutputFolder) {
    Write-Host '==> taking the screenshots' -ForegroundColor Cyan
    $socket = Connect-DemoPage $Port
    # Wait (up to a minute) for the screens to load and the collector banner to settle.
    $state = 'none'
    $deadline = (Get-Date).AddSeconds(60)
    while ($state -in @('none', 'unknown', '') -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        $state = Invoke-PageScript $socket "document.querySelector('[role=status]')?.getAttribute('data-state') ?? 'none'"
    }
    Write-Host "  collector banner state: $state"
    if ($state -ne 'running') {
        $where = Invoke-PageScript $socket "location.href + ' | ' + document.title"
        $text = Invoke-PageScript $socket "document.body.innerText.replace(/\s+/g, ' ').slice(0, 300)"
        throw "The demo collector should read as running, but the banner says '$state'. Page: $where. Text: $text"
    }

    # No page scrollbar in the pictures.
    Invoke-PageScript $socket "(() => { const s = document.createElement('style'); s.textContent = '::-webkit-scrollbar { display: none; }'; document.head.appendChild(s); return 'ok'; })()" | Out-Null
    Click-DemoButton $socket 'Overview'
    Save-DemoShot $socket $OutputFolder 'overview' 1000
    Click-DemoButton $socket 'Findings'
    Save-DemoShot $socket $OutputFolder 'findings' 1100
    Click-DemoButton $socket 'Processes'
    Save-DemoShot $socket $OutputFolder 'processes' 800
    Click-DemoButton $socket 'Connections'
    Save-DemoShot $socket $OutputFolder 'connections' 800
    Click-DemoButton $socket 'History'
    Save-DemoShot $socket $OutputFolder 'history-system' 1300
    Click-DemoButton $socket 'Programs' -Exact
    Click-DemoButton $socket 'GPU' -Exact
    Save-DemoShot $socket $OutputFolder 'history-programs-gpu' 1000 1760
    Click-DemoButton $socket 'toolx.exe' -Exact
    # Choosing a program scrolls its details into view; the picture shows the whole page, the list above them included.
    Invoke-PageScript $socket "(() => { window.scrollTo(0, 0); return 'ok'; })()" | Out-Null
    Save-DemoShot $socket $OutputFolder 'history-program-detail' 1700 1760
    Click-DemoButton $socket 'Network' -Exact
    Save-DemoShot $socket $OutputFolder 'history-network' 1200
    Click-DemoButton $socket 'Known programs' -Exact
    Save-DemoShot $socket $OutputFolder 'known-programs' 900
    $socket.Dispose()
}
