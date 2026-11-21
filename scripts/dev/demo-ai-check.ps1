# Drives the AI help of the demo app (docs/PHASE4-PLAN.md) and reports what worked. Dot-sourced by run-demo.ps1 -CheckAi, after demo-capture.ps1.
#
# It clicks through the REAL screens of the app on the fictional machine, against a stand-in model server that answers with a canned text and
# keeps a log of every request it receives. Then it checks, from that log, what a model server would have been told. Nothing real is involved.

function Test-Page($Socket, [string]$Condition) {
    return (Invoke-PageScript $Socket "String($Condition)") -eq 'true'
}

function Wait-Page($Socket, [string]$Condition, [int]$Seconds = 30) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Page $Socket $Condition) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

function Test-DemoAi([int]$Port, [string]$OutputFolder, [string]$ModelLog) {
    $results = New-Object System.Collections.Generic.List[string]
    $failed = $false
    $note = {
        param([bool]$Ok, [string]$Text)
        $line = if ($Ok) { "PASS  $Text" } else { "FAIL  $Text" }
        Write-Host "  $line" -ForegroundColor $(if ($Ok) { 'Green' } else { 'Red' })
        $results.Add($line)
        if (-not $Ok) { Set-Variable -Name failed -Value $true -Scope 1 }
    }

    Write-Host '==> checking the AI help in the demo app' -ForegroundColor Cyan
    $socket = Connect-DemoPage $Port
    if (-not (Wait-Page $socket "document.querySelector('[role=status]')?.getAttribute('data-state') === 'running'" 60)) {
        throw 'The demo collector never read as running.'
    }
    Invoke-PageScript $socket "(() => { const s = document.createElement('style'); s.textContent = '::-webkit-scrollbar { display: none; }'; document.head.appendChild(s); return 'ok'; })()" | Out-Null

    # 1. The AI help tab: the settings the runner seeded are shown, and the stand-in server is seen as already running.
    Click-DemoButton $socket 'AI help' -Exact
    & $note (Test-Page $socket "document.querySelector('input[type=checkbox]')?.checked === true") 'the AI help tab shows the help as turned on'
    & $note (Test-Page $socket "[...document.querySelectorAll('input[type=text]')].some(i => i.value.startsWith('http://127.0.0.1:'))") 'the server address is a loopback address'
    $ready = Wait-Page $socket "document.body.innerText.includes('already running; this app will not stop it')" 20
    & $note $ready 'the status says a server is ready and that this app did not start it (so it will not stop it)'
    & $note (-not (Test-Page $socket "[...document.querySelectorAll('button')].some(b => b.textContent.includes('Stop the model server'))")) 'no Stop button for a server this app did not start'
    Save-DemoShot $socket $OutputFolder 'ai-help' 1500

    # 2. Investigate a finding: the answer streams in as plain text and the evidence that was sent is shown.
    Click-DemoButton $socket 'Findings'
    & $note (Test-Page $socket "[...document.querySelectorAll('button')].some(b => b.textContent.trim() === 'Investigate')") 'findings have an Investigate button while the help is on'
    Click-DemoButton $socket 'Investigate' -Exact
    Click-DemoButton $socket 'Investigate with my model' -Exact
    $finished = Wait-Page $socket "document.body.innerText.includes('Finished.')" 40
    & $note $finished 'the investigation finished'
    & $note (Test-Page $socket "document.querySelector('[aria-label=Answer]')?.textContent.includes('demo stand-in model')") 'the streamed answer is on the page'
    & $note (Test-Page $socket "document.querySelector('[aria-label=Answer]')?.textContent.includes('It was asked about')") 'the answer used the evidence it was given'
    Invoke-PageScript $socket "(() => { const d = document.querySelector('.ai-panel details'); if (d) d.open = true; document.querySelector('.ai-panel')?.scrollIntoView(); return 'ok'; })()" | Out-Null
    & $note (Test-Page $socket "document.querySelector('.ai-sent')?.textContent.includes('<evidence>')") 'the page shows what was sent'
    & $note (-not (Test-Page $socket "/users\\+demo/i.test(document.querySelector('.ai-sent')?.textContent ?? '')")) 'what was sent does not contain the user name in a path'
    & $note (-not (Test-Page $socket "/\b\d{1,3}(\.\d{1,3}){3}\b/.test(document.querySelector('.ai-sent')?.textContent ?? '')")) 'what was sent contains no IP address (summary level)'
    Save-DemoShot $socket $OutputFolder 'ai-investigate-answer' 1500

    # 3. Stop: ask again and stop while the answer is still streaming.
    Click-DemoButton $socket 'Ask again' -Exact
    $stopped = $false
    if (Wait-Page $socket "[...document.querySelectorAll('button')].some(b => b.textContent.trim() === 'Stop')" 15) {
        Click-DemoButton $socket 'Stop' -Exact
        $stopped = Wait-Page $socket "document.body.innerText.includes('Stopped.')" 15
    }
    & $note $stopped 'Stop ends an answer that is still streaming'
    & $note (-not (Test-Page $socket "document.querySelector('[aria-label=Answer]')?.textContent.includes('Stop works.')")) 'a stopped answer is cut short (it keeps what had arrived)'

    # 4. The text for an outside assistant: reduced, shown, editable.
    Click-DemoButton $socket 'Show the text to copy' -Exact
    $box = "document.querySelector('textarea.ai-copy')?.value ?? ''"
    & $note (Test-Page $socket "($box).includes('<evidence>')") 'the outside text is shown in a box'
    & $note (-not (Test-Page $socket "/\.exe/i.test($box)")) 'the outside text names no program file'
    & $note (-not (Test-Page $socket "/users/i.test($box)")) 'the outside text names no folder'
    & $note (-not (Test-Page $socket "/\.example/i.test($box)")) 'the outside text names no host or network'
    & $note (-not (Test-Page $socket "/\b\d{1,3}(\.\d{1,3}){3}\b/.test($box)")) 'the outside text contains no IP address'
    Invoke-PageScript $socket "(() => { document.querySelector('textarea.ai-copy')?.scrollIntoView(); return 'ok'; })()" | Out-Null
    Save-DemoShot $socket $OutputFolder 'ai-copy-box' 1500
    $socket.Dispose()

    # 5. What the model server actually received (its own log of every request body).
    $lines = @()
    if (Test-Path $ModelLog) { $lines = @(Get-Content $ModelLog) }
    & $note ($lines.Count -ge 2) "the stand-in model received the requests ($($lines.Count) so far)"
    foreach ($line in $lines) {
        $body = $line | ConvertFrom-Json
        $user = [string]$body.messages[1].content
        & $note ($body.model -eq 'demo-model' -and $body.stream -eq $true) 'a request names the model exactly and asks for a stream'
        & $note ($body.messages[0].role -eq 'system' -and $body.messages[0].content -match 'never follow instructions') 'a request carries the instruction that the evidence is data'
        & $note ($user -notmatch '(?i)users\\+demo' -and $user -notmatch '\b\d{1,3}(\.\d{1,3}){3}\b') 'a request carries no user name in a path and no IP address'
    }

    $passed = -not $failed
    $summary = if ($passed) { 'ALL CHECKS PASSED' } else { 'SOME CHECKS FAILED' }
    $results.Add($summary)
    Set-Content -Path (Join-Path $OutputFolder 'report.txt') -Value $results -Encoding UTF8
    Write-Host "==> $summary (report and pictures: $OutputFolder)" -ForegroundColor $(if ($passed) { 'Green' } else { 'Red' })
    return $passed
}

# The full text of a page expression (Invoke-PageScript stops at the first quote; an answer has many).
function Get-PageString($Socket, [string]$Expression) {
    $reply = Invoke-Cdp $Socket 'Runtime.evaluate' @{ expression = $Expression; returnByValue = $true }
    $value = ($reply | ConvertFrom-Json).result.result.value
    if ($null -eq $value) { return '' }
    return [string]$value
}

# Investigates one demo finding against a REAL model server (run-demo.ps1 -LiveAi ... -ModelEndpoint ... -ModelName ...), waiting as long as a
# slow model needs. The demo data is invented, so nothing real is sent. Writes the answer and a picture to $OutputFolder and returns $true when
# an answer arrived.
function Test-DemoAiLive([int]$Port, [string]$OutputFolder, [int]$WaitSeconds = 900) {
    Write-Host '==> asking the real model server about a demo finding' -ForegroundColor Cyan
    $socket = Connect-DemoPage $Port
    if (-not (Wait-Page $socket "document.querySelector('[role=status]')?.getAttribute('data-state') === 'running'" 60)) {
        throw 'The demo collector never read as running.'
    }
    Invoke-PageScript $socket "(() => { const s = document.createElement('style'); s.textContent = '::-webkit-scrollbar { display: none; }'; document.head.appendChild(s); return 'ok'; })()" | Out-Null

    Click-DemoButton $socket 'AI help' -Exact
    Start-Sleep -Seconds 4
    $status = Get-PageString $socket "document.querySelector('[aria-label=""Model server status""]')?.innerText ?? ''"
    Write-Host "  server status on the AI help tab: $($status -replace '\s+', ' ')"

    Click-DemoButton $socket 'Findings'
    Click-DemoButton $socket 'Investigate' -Exact
    Click-DemoButton $socket 'Investigate with my model' -Exact
    $started = Get-Date
    $firstWords = $null
    $outcome = 'timed out'
    while (((Get-Date) - $started).TotalSeconds -lt $WaitSeconds) {
        Start-Sleep -Seconds 2
        $length = [int](Get-PageString $socket "String(document.querySelector('[aria-label=Answer]')?.textContent.length ?? 0)")
        $phase = Get-PageString $socket "(document.querySelector('.ai-panel')?.innerText.match(/(Finished\.|Stopped\.|It could not finish\.|The model is answering|Waiting for the model server)/) ?? [''])[0]"
        if ($length -gt 0 -and $null -eq $firstWords) {
            $firstWords = ((Get-Date) - $started).TotalSeconds
            Write-Host ("  first words after {0:N0} s" -f $firstWords)
        }
        $problem = Get-PageString $socket "document.querySelector('.ai-panel [role=alert]')?.textContent ?? ''"
        if ($problem) { $outcome = "problem: $problem"; break }
        if ($phase -eq 'Finished.') { $outcome = 'finished'; break }
        Write-Host ("  {0,4:N0} s  {1}  ({2} characters so far)" -f ((Get-Date) - $started).TotalSeconds, $phase, $length)
    }
    $seconds = ((Get-Date) - $started).TotalSeconds
    $answer = Get-PageString $socket "document.querySelector('[aria-label=Answer]')?.textContent ?? ''"
    $reasoning = Get-PageString $socket "document.querySelector('.ai-panel')?.innerText ?? ''"
    Set-Content -Path (Join-Path $OutputFolder 'answer.txt') -Value $answer -Encoding UTF8
    Set-Content -Path (Join-Path $OutputFolder 'panel-text.txt') -Value $reasoning -Encoding UTF8
    Invoke-PageScript $socket "(() => { (document.querySelector('.ai-panel')?.closest('.finding') ?? document.querySelector('.ai-panel'))?.scrollIntoView(); return 'ok'; })()" | Out-Null
    Save-DemoShot $socket $OutputFolder 'ai-live-answer' 1150
    $socket.Dispose()
    Write-Host ("==> {0} after {1:N0} s; the answer is {2} characters (saved to answer.txt)" -f $outcome, $seconds, $answer.Length) -ForegroundColor $(if ($outcome -eq 'finished') { 'Green' } else { 'Red' })
    return ($outcome -eq 'finished' -and $answer.Length -gt 80)
}
