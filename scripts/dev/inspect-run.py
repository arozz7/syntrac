"""Read-only report on a SYNTRAC collector database: is the data complete, attributed and consistent?

Usage:  python -I scripts/dev/inspect-run.py <path-to-database>

Opens the database read-only, so it is safe to run while the collector is still writing.
"""

import sqlite3
import sys


def share(part, whole):
    return "n/a" if not whole else f"{part}/{whole} ({100.0 * part / whole:.0f}%)"


def main(path):
    con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    one = lambda sql, *args: con.execute(sql, args).fetchone()[0]

    print(f"schema version: {one('pragma user_version')}")

    print("\n== events by kind ==")
    for kind, count in con.execute("select kind, count(*) from events group by kind order by 2 desc"):
        print(f"  {kind:<18} {count}")

    print("\n== attribution (events/rows that resolved to a process instance) ==")
    for kind in ("network_flow", "dns_query", "process_exited"):
        total = one("select count(*) from events where kind = ?", kind)
        attributed = one("select count(*) from events where kind = ? and process_instance_id is not null", kind)
        print(f"  {kind:<16} {share(attributed, total)}")
    metric_total = one("select count(*) from process_metrics")
    metric_resolved = one("select count(*) from process_metrics where instance is not null")
    print(f"  process_metrics  {share(metric_resolved, metric_total)}")

    print("\n== flows: duplicate rows for the same window (should be 0) ==")
    duplicates = one(
        "select count(*) from (select 1 from flows group by window_start, process_instance_id, pid,"
        " protocol, remote_addr, remote_port having count(*) > 1)"
    )
    print(f"  duplicate flow keys: {duplicates}")

    print("\n== processes ==")
    observed = one("select count(*) from events where kind = 'process_observed'")
    started = one("select count(*) from events where kind = 'process_started'")
    exited = one("select count(*) from events where kind = 'process_exited'")
    print(f"  observed at startup: {observed}   started while running: {started}   exited: {exited}")
    print(f"  still marked running: {one('select count(*) from processes where ended is null')}")
    unmatched_exits = one(
        "select count(*) from events where kind = 'process_exited' and process_instance_id is null"
    )
    print(f"  exits we could not match to a known process: {unmatched_exits}")
    print("  (the startup snapshot's skipped count is in the console summary file)")

    print("\n== CPU: who used it? (whole-machine share, averaged over the run) ==")
    samples = one("select count(*) from system_metrics")
    if samples:
        system_avg = one("select avg(cpu_percent) from system_metrics")
        tail_avg = one("select avg(others_cpu) from system_metrics")
        listed_avg = (one("select coalesce(sum(cpu_percent), 0) from process_metrics") or 0) / samples
        unaccounted = max(0.0, system_avg - listed_avg - tail_avg)
        print(f"  samples: {samples}   system: {system_avg:.1f}%   listed processes: {listed_avg:.1f}%"
              f"   long tail: {tail_avg:.1f}%   NOT ATTRIBUTED: {unaccounted:.1f}%")
        print("  heaviest processes:")
        rows = con.execute(
            "select m.pid, coalesce(p.image_path, '(unknown process)'), sum(m.cpu_percent) / ? as avg_cpu, max(m.cpu_percent)"
            " from process_metrics m left join processes p on p.id = m.instance"
            " group by m.instance, m.pid order by avg_cpu desc limit 8",
            (samples,),
        )
        for pid, image, avg_cpu, peak in rows:
            print(f"    {avg_cpu:5.2f}% avg  {peak:5.2f}% peak  pid {pid:<7} {image.split(chr(92))[-1]}")
    else:
        print("  no metrics samples")

    print("\n== network ==")
    print(f"  flows: {one('select count(*) from flows')}   distinct remote endpoints: "
          f"{one('select count(*) from (select 1 from flows group by remote_addr, remote_port)')}")
    print("  busiest destinations (by bytes):")
    for addr, port, sent, received, flows in con.execute(
        "select remote_addr, remote_port, sum(bytes_sent), sum(bytes_received), count(*) from flows"
        " group by 1, 2 order by sum(bytes_sent) + sum(bytes_received) desc limit 6"
    ):
        print(f"    {addr}:{port:<6} sent {sent:>10}  received {received:>10}  flows {flows}")

    print("\n== DNS ==")
    print(f"  lookups: {one('select count(*) from events where kind = ?', 'dns_query')}")
    for payload, in con.execute("select payload from events where kind = 'dns_query' order by ts desc limit 3"):
        print(f"    {payload[:150]}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
