//! Prints how many bytes each table and index of a database occupies (read-only; uses SQLite's `dbstat`).
//! Usage: cargo run -p syntrac-store --example table_sizes -- <database-file>
use rusqlite::{Connection, OpenFlags};

fn main() {
    let path = std::env::args().nth(1).expect("usage: table_sizes <database-file>");
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open read-only");
    let mut statement = conn
        .prepare("SELECT name, SUM(pgsize), COUNT(*) FROM dbstat GROUP BY name ORDER BY SUM(pgsize) DESC")
        .expect("dbstat is available");
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))
        .expect("query");
    let mut total = 0;
    for row in rows {
        let (name, bytes, pages) = row.expect("row");
        total += bytes;
        println!("{name:<40} {:>10.1} MB {pages:>8} pages", bytes as f64 / 1_048_576.0);
    }
    println!("{:<40} {:>10.1} MB", "TOTAL", total as f64 / 1_048_576.0);
}
