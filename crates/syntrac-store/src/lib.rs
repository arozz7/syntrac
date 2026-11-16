//! SQLite persistence for SYNTRAC. Business logic depends on the repository traits in
//! `syntrac-core`, never on this crate directly.

mod codec;
mod connection;
mod contexts;
mod destinations;
mod events;
mod identities;
mod inspect;
mod known_products;
mod launchers;
mod metrics;
#[cfg(test)]
mod migration_v10_tests;
#[cfg(test)]
mod migration_v11_tests;
#[cfg(test)]
mod migration_v4_tests;
#[cfg(test)]
mod migration_v5_tests;
#[cfg(test)]
mod migration_v6_tests;
#[cfg(test)]
mod migration_v7_tests;
#[cfg(test)]
mod migration_v8_tests;
#[cfg(test)]
mod migration_v9_tests;
mod migrations;
mod names;
mod owners;
mod processes;
mod rollups;
mod storage;
#[cfg(test)]
mod storage_tests;
mod trends;

pub use connection::SqliteStore;
pub use inspect::{inspect_database, DatabaseReport};
