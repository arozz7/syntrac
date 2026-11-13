//! SQLite persistence for SYNTRAC. Business logic depends on the repository traits in
//! `syntrac-core`, never on this crate directly.

mod codec;
mod connection;
mod events;
mod inspect;
mod metrics;
mod migrations;
mod processes;

pub use connection::SqliteStore;
pub use inspect::{inspect_database, DatabaseReport};
