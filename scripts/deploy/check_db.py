"""Reads a SYNTRAC database the way the UI will: read-only, as an ordinary user.

Usage:  python -I scripts/deploy/check_db.py <path-to-database>

Prints one line: <latest metrics ts> <latest event ts> <schema version> <running processes> <approximate exits>
Timestamps are microseconds since the Unix epoch; 0 means "none yet". A non-zero exit code means the database
could not be opened or read read-only, which is exactly what the smoke test needs to detect.
"""

import sqlite3
import sys


def main(path):
    con = sqlite3.connect("file:" + path + "?mode=ro", uri=True)
    metrics = con.execute("select max(ts) from system_metrics").fetchone()[0] or 0
    events = con.execute("select max(ts) from events").fetchone()[0] or 0
    version = con.execute("pragma user_version").fetchone()[0]
    running = con.execute("select count(*) from processes where ended is null").fetchone()[0]
    approximate = con.execute("select count(*) from processes where ended_approximate = 1").fetchone()[0]
    print(metrics, events, version, running, approximate)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
