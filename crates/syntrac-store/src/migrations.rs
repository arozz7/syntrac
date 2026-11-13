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
];
