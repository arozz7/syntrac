//! Ordered, append-only schema migrations. The applied count is kept in `PRAGMA user_version`.
//! Never edit a released migration; add a new one.

pub const MIGRATIONS: &[&str] = &[
    // 1: process table and event log
    "
    CREATE TABLE processes (
        id          TEXT PRIMARY KEY,
        pid         INTEGER NOT NULL,
        parent_pid  INTEGER NOT NULL,
        parent_id   TEXT,
        image_path  TEXT NOT NULL,
        command_line TEXT,
        started     INTEGER NOT NULL,
        ended       INTEGER,
        exit_code   INTEGER
    );
    CREATE INDEX idx_processes_pid_started ON processes (pid, started);

    CREATE TABLE events (
        id          TEXT PRIMARY KEY,
        ts          INTEGER NOT NULL,
        category    TEXT NOT NULL,
        kind        TEXT NOT NULL,
        process_instance_id TEXT,
        severity    TEXT,
        payload     TEXT NOT NULL
    );
    CREATE INDEX idx_events_ts ON events (ts);
    CREATE INDEX idx_events_category_ts ON events (category, ts);
    CREATE INDEX idx_events_process_ts ON events (process_instance_id, ts);
    ",
    // 2: indexed network flows (also kept in `events` for the timeline) and resource metrics
    "
    CREATE TABLE flows (
        event_id    TEXT PRIMARY KEY,
        window_start INTEGER NOT NULL,
        window_end  INTEGER NOT NULL,
        process_instance_id TEXT,
        pid         INTEGER NOT NULL,
        protocol    TEXT NOT NULL,
        remote_addr TEXT NOT NULL,
        remote_port INTEGER NOT NULL,
        bytes_sent  INTEGER NOT NULL,
        bytes_received INTEGER NOT NULL,
        opened      INTEGER NOT NULL,
        closed      INTEGER NOT NULL,
        failed      INTEGER NOT NULL
    );
    CREATE INDEX idx_flows_window ON flows (window_start);
    CREATE INDEX idx_flows_process ON flows (process_instance_id, window_start);

    CREATE TABLE system_metrics (
        ts          INTEGER PRIMARY KEY,
        cpu_percent REAL NOT NULL,
        memory_used INTEGER NOT NULL,
        memory_total INTEGER NOT NULL,
        disk_read   INTEGER NOT NULL,
        disk_write  INTEGER NOT NULL,
        net_received INTEGER NOT NULL,
        net_sent    INTEGER NOT NULL,
        others_count INTEGER NOT NULL,
        others_cpu  REAL NOT NULL,
        others_memory INTEGER NOT NULL
    );

    CREATE TABLE process_metrics (
        ts          INTEGER NOT NULL,
        instance    TEXT,
        pid         INTEGER NOT NULL,
        cpu_percent REAL NOT NULL,
        memory_bytes INTEGER NOT NULL
    );
    CREATE INDEX idx_process_metrics_ts ON process_metrics (ts);
    CREATE INDEX idx_process_metrics_instance_ts ON process_metrics (instance, ts);
    ",
    // 3: a process closed by a startup reconciliation has only an upper bound on its exit time
    "
    ALTER TABLE processes ADD COLUMN ended_approximate INTEGER NOT NULL DEFAULT 0;
    ",
    // 4: compact layout (docs/STORAGE-PLAN.md, S2). Ids become 16-byte blobs (unhex of the text id without dashes; a malformed
    // event or process id gives NULL, which fails its NOT NULL column and rolls the whole migration back instead of corrupting
    // anything). A flow is stored once, in `flows`, keyed by the same number as its event (`seq`); its event keeps a NULL
    // payload, but only when its flows row exists, so no event can lose its content. Three indexes that no query plan uses are
    // dropped (found with EXPLAIN QUERY PLAN on a copy of the real database).
    "
    CREATE TABLE events_v4 (
        seq         INTEGER PRIMARY KEY,
        id          BLOB NOT NULL UNIQUE,
        ts          INTEGER NOT NULL,
        category    TEXT NOT NULL,
        kind        TEXT NOT NULL,
        process_instance_id BLOB,
        severity    TEXT,
        payload     TEXT
    );
    INSERT INTO events_v4 (seq, id, ts, category, kind, process_instance_id, severity, payload)
    SELECT e.rowid, unhex(replace(e.id, '-', '')), e.ts, e.category, e.kind,
           unhex(replace(e.process_instance_id, '-', '')), e.severity,
           CASE WHEN EXISTS (SELECT 1 FROM flows f WHERE f.event_id = e.id) THEN NULL ELSE e.payload END
    FROM events e ORDER BY e.rowid;

    CREATE TABLE flows_v4 (
        seq         INTEGER PRIMARY KEY,
        window_start INTEGER NOT NULL,
        window_end  INTEGER NOT NULL,
        process_instance_id BLOB,
        pid         INTEGER NOT NULL,
        protocol    TEXT NOT NULL,
        remote_addr TEXT NOT NULL,
        remote_port INTEGER NOT NULL,
        bytes_sent  INTEGER NOT NULL,
        bytes_received INTEGER NOT NULL,
        opened      INTEGER NOT NULL,
        closed      INTEGER NOT NULL,
        failed      INTEGER NOT NULL
    );
    INSERT INTO flows_v4 (seq, window_start, window_end, process_instance_id, pid, protocol, remote_addr, remote_port,
                          bytes_sent, bytes_received, opened, closed, failed)
    SELECT v.seq, f.window_start, f.window_end, unhex(replace(f.process_instance_id, '-', '')), f.pid, f.protocol,
           f.remote_addr, f.remote_port, f.bytes_sent, f.bytes_received, f.opened, f.closed, f.failed
    FROM flows f JOIN events_v4 v ON v.id = unhex(replace(f.event_id, '-', ''))
    ORDER BY v.seq;

    CREATE TABLE processes_v4 (
        id          BLOB PRIMARY KEY,
        pid         INTEGER NOT NULL,
        parent_pid  INTEGER NOT NULL,
        parent_id   BLOB,
        image_path  TEXT NOT NULL,
        command_line TEXT,
        started     INTEGER NOT NULL,
        ended       INTEGER,
        exit_code   INTEGER,
        ended_approximate INTEGER NOT NULL DEFAULT 0
    );
    INSERT INTO processes_v4 (id, pid, parent_pid, parent_id, image_path, command_line, started, ended, exit_code,
                              ended_approximate)
    SELECT unhex(replace(id, '-', '')), pid, parent_pid, unhex(replace(parent_id, '-', '')), image_path, command_line,
           started, ended, exit_code, ended_approximate
    FROM processes ORDER BY rowid;

    CREATE TABLE process_metrics_v4 (
        ts          INTEGER NOT NULL,
        instance    BLOB,
        pid         INTEGER NOT NULL,
        cpu_percent REAL NOT NULL,
        memory_bytes INTEGER NOT NULL
    );
    INSERT INTO process_metrics_v4 (ts, instance, pid, cpu_percent, memory_bytes)
    SELECT ts, unhex(replace(instance, '-', '')), pid, cpu_percent, memory_bytes FROM process_metrics ORDER BY rowid;

    DROP TABLE flows;
    DROP TABLE events;
    DROP TABLE processes;
    DROP TABLE process_metrics;
    ALTER TABLE events_v4 RENAME TO events;
    ALTER TABLE flows_v4 RENAME TO flows;
    ALTER TABLE processes_v4 RENAME TO processes;
    ALTER TABLE process_metrics_v4 RENAME TO process_metrics;

    CREATE INDEX idx_events_ts ON events (ts);
    CREATE INDEX idx_events_category_ts ON events (category, ts);
    CREATE INDEX idx_events_process_ts ON events (process_instance_id, ts);
    CREATE INDEX idx_flows_window ON flows (window_start);
    CREATE INDEX idx_processes_pid_started ON processes (pid, started);
    CREATE INDEX idx_process_metrics_ts ON process_metrics (ts);
    ",
    // 5: hourly rollups and permanent ledgers (docs/STORAGE-PLAN.md, S3). Additive: nothing existing is touched. Hours are
    // UTC. The traffic key has no port (most ports are ephemeral); `top_port` and `distinct_ports` summarise them.
    // `executables` is both the dimension table (its id is used by the rollup rows) and the executable ledger; row 1 stands
    // for 'unattributed' (owner or path unknown) and has no sightings.
    "
    CREATE TABLE executables (
        id          INTEGER PRIMARY KEY,
        path_key    TEXT NOT NULL UNIQUE,
        path        TEXT NOT NULL,
        first_seen  INTEGER,
        last_seen   INTEGER
    );
    INSERT INTO executables (id, path_key, path) VALUES (1, '', '(unattributed)');

    CREATE TABLE destinations (
        addr        TEXT PRIMARY KEY,
        first_seen  INTEGER NOT NULL,
        last_seen   INTEGER NOT NULL
    ) WITHOUT ROWID;

    CREATE TABLE exe_destinations (
        exe_id      INTEGER NOT NULL,
        addr        TEXT NOT NULL,
        first_seen  INTEGER NOT NULL,
        last_seen   INTEGER NOT NULL,
        PRIMARY KEY (exe_id, addr)
    ) WITHOUT ROWID;

    CREATE TABLE rollup_system_hour (
        hour_start  INTEGER PRIMARY KEY,
        samples     INTEGER NOT NULL,
        cpu_avg     REAL NOT NULL,
        cpu_max     REAL NOT NULL,
        memory_used_avg INTEGER NOT NULL,
        memory_used_max INTEGER NOT NULL,
        memory_total INTEGER NOT NULL,
        disk_read   INTEGER NOT NULL,
        disk_write  INTEGER NOT NULL,
        net_received INTEGER NOT NULL,
        net_sent    INTEGER NOT NULL,
        others_cpu_avg REAL NOT NULL
    );

    CREATE TABLE rollup_process_hour (
        hour_start  INTEGER NOT NULL,
        exe_id      INTEGER NOT NULL,
        samples_present INTEGER NOT NULL,
        cpu_avg     REAL NOT NULL,
        cpu_peak    REAL NOT NULL,
        memory_peak INTEGER NOT NULL,
        starts      INTEGER NOT NULL,
        first_seen  INTEGER NOT NULL,
        last_seen   INTEGER NOT NULL,
        PRIMARY KEY (hour_start, exe_id)
    ) WITHOUT ROWID;
    CREATE INDEX idx_rollup_process_exe ON rollup_process_hour (exe_id, hour_start);

    CREATE TABLE rollup_traffic_hour (
        hour_start  INTEGER NOT NULL,
        exe_id      INTEGER NOT NULL,
        addr        TEXT NOT NULL,
        bytes_sent  INTEGER NOT NULL,
        bytes_received INTEGER NOT NULL,
        flows       INTEGER NOT NULL,
        first_seen  INTEGER NOT NULL,
        last_seen   INTEGER NOT NULL,
        distinct_ports INTEGER NOT NULL,
        top_port    INTEGER NOT NULL,
        PRIMARY KEY (hour_start, exe_id, addr)
    ) WITHOUT ROWID;
    CREATE INDEX idx_rollup_traffic_addr ON rollup_traffic_hour (addr, hour_start);

    CREATE TABLE rollup_progress (
        id          INTEGER PRIMARY KEY CHECK (id = 1),
        hours_done_before INTEGER NOT NULL
    );
    ",
    // 6: what the files themselves tell us about each executable (docs/IDENTITY-PLAN.md, I1a). Additive: nothing existing is
    // touched. One row per executable, keyed like the rollups. Permanent: the purge of detail never touches it. A row with
    // size NULL has no facts yet (the first look failed); ailure_kind NULL means the last look succeeded.
    "
    CREATE TABLE executable_identity (
        exe_id           INTEGER PRIMARY KEY,
        last_attempt     INTEGER NOT NULL,
        inspected_at     INTEGER,
        failure_kind     TEXT,
        failure_message  TEXT,
        failure_attempts INTEGER NOT NULL DEFAULT 0,
        size             INTEGER,
        modified         INTEGER,
        sha256           TEXT,
        signature_status TEXT,
        signer           TEXT,
        thumbprint       TEXT,
        signature_kind   TEXT,
        company          TEXT,
        product          TEXT,
        description      TEXT,
        original_filename TEXT,
        file_version     TEXT,
        zone_id          INTEGER
    );
    ",
    // 7: products the user has confirmed (docs/KNOWN-PROGRAMS-DESIGN.md, I3). Additive. publisher is '' for a product with
    // no publisher, so (publisher, root) is unique for those too. Permanent: the purge of detail never touches it.
    "
    CREATE TABLE known_products (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        label        TEXT NOT NULL,
        publisher    TEXT NOT NULL DEFAULT '',
        root         TEXT NOT NULL,
        product_name TEXT,
        note         TEXT,
        created_at   INTEGER NOT NULL,
        updated_at   INTEGER NOT NULL,
        UNIQUE (publisher, root)
    );
    ",
    // 8: which names were asked for when an address was reached (docs/NETWORK-PLAN.md, T3a). Additive. One row per (address,
    // name) pair ever seen in a DNS answer, with first and last seen only (so writing is idempotent). The raw DNS events are
    // purged after the retention window; this is what stays. `names_progress` has a row once the one-time backfill from the
    // retained DNS events has been done. Permanent: the purge of detail never touches either table.
    "
    CREATE TABLE address_names (
        addr        TEXT NOT NULL,
        name        TEXT NOT NULL,
        first_seen  INTEGER NOT NULL,
        last_seen   INTEGER NOT NULL,
        PRIMARY KEY (addr, name)
    ) WITHOUT ROWID;

    CREATE TABLE names_progress (
        id          INTEGER PRIMARY KEY CHECK (id = 1)
    );
    ",
];

/// The migration (1-based number) that rewrites every table. An existing database gets a backup copy before it is applied
/// and is compacted (`VACUUM`) after it, because a rewrite leaves free pages and the file would not shrink otherwise.
pub const REWRITING_MIGRATION: u32 = 4;

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;

    #[test]
    fn upgrading_a_version_1_database_keeps_its_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.execute(
                "INSERT INTO events (id, ts, category, kind, process_instance_id, severity, payload)
                 VALUES ('00000000-0000-0000-0000-000000000001', 5, 'dns', 'dns_query', NULL, NULL, '{\"kind\":\"dns_query\",\"pid\":1,\"query\":\"a\",\"addresses\":[]}')",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }

        let store = crate::SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), crate::SqliteStore::LATEST_SCHEMA_VERSION);
        let events = syntrac_core::EventRepository::query(&store, &syntrac_core::EventQuery::default()).unwrap();
        assert_eq!(events.len(), 1, "data from version 1 survives the upgrade");
    }
}
