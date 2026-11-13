//! SQLite persistence for SYNTRAC. Business logic depends on the repository traits in
//! `syntrac-core`, never on this crate directly.

mod migrations;
mod sqlite;

pub use sqlite::SqliteStore;
