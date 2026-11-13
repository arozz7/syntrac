//! Runs one read-only SQL query and prints the rows tab-separated. A diagnostic for operators and for design measurements.
//! Usage: cargo run -p syntrac-store --example sql_query -- <database-file> "<select ...>"
use rusqlite::{types::ValueRef, Connection, OpenFlags};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: sql_query <database-file> <sql>");
    let sql = args.next().expect("usage: sql_query <database-file> <sql>");
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open read-only");
    let mut statement = conn.prepare(&sql).expect("prepare");
    let columns = statement.column_count();
    let mut rows = statement.query([]).expect("query");
    while let Some(row) = rows.next().expect("row") {
        let cells: Vec<String> = (0..columns)
            .map(|i| match row.get_ref(i).expect("cell") {
                ValueRef::Null => "NULL".to_owned(),
                ValueRef::Integer(v) => v.to_string(),
                ValueRef::Real(v) => format!("{v:.3}"),
                ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                ValueRef::Blob(b) => format!("blob[{}]", b.len()),
            })
            .collect();
        println!("{}", cells.join("\t"));
    }
}
