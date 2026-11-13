# The Phase 0 ETW spike (the etw_spike binary) was removed in Phase 1a: the console collector superseded it.
# Use scripts/dev/run-console.ps1 instead. Its results are kept in aiChangeLog/etw-spike-result-kw*.txt.
Write-Host 'The ETW spike was removed in Phase 1a. Use scripts/dev/run-console.ps1.' -ForegroundColor Yellow
exit 1
