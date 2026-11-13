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
    CREATE INDEX idx_flows_remote ON flows (remote_addr, remote_port, window_start);
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
];

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
