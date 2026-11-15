# aiChangeLog

One `phase-*.md` per phase: what changed, the tests written first, what was measured, and the limits. These are the record of the work.

Some phase documents mention raw run output (`service-smoke-*.txt`, `console-run-*.txt`, `overhead-*.txt`, `*-rehearsal-output.txt`,
`etw-spike-result-*.txt`). Those files are **kept on the developer's machine only and are not published**: they name the machine and
the Windows account and list the programs that were running. They are listed in `.gitignore` so they cannot be committed by accident.
The phase documents carry the results that matter; a new run of `scripts/deploy/service-smoke.ps1` or `scripts/dev/run-console.ps1`
writes a fresh file here.
