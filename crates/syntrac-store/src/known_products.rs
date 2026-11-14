//! `KnownProductRepository` for SQLite: the products the user has confirmed. Permanent: the purge of detail never touches them.

use rusqlite::{params, Row};
use syntrac_core::{KnownProduct, KnownProductRepository, NewKnownProduct, RepoError, Timestamp};

use crate::connection::{backend, SqliteStore};

/// `publisher` is stored as '' for "no publisher", so the uniqueness of (publisher, root) also holds for those products.
fn publisher_text(publisher: &Option<String>) -> &str {
    publisher.as_deref().unwrap_or("")
}

fn product_from(row: &Row<'_>) -> Result<KnownProduct, RepoError> {
    let publisher: String = row.get(2).map_err(backend)?;
    Ok(KnownProduct {
        id: row.get(0).map_err(backend)?,
        label: row.get(1).map_err(backend)?,
        publisher: if publisher.is_empty() { None } else { Some(publisher) },
        root: row.get(3).map_err(backend)?,
        product_name: row.get(4).map_err(backend)?,
        note: row.get(5).map_err(backend)?,
        created_at: Timestamp::from_micros(row.get(6).map_err(backend)?),
    })
}

impl KnownProductRepository for SqliteStore {
    fn known_products(&self) -> Result<Vec<KnownProduct>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, label, publisher, root, product_name, note, created_at FROM known_products ORDER BY id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut products = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            products.push(product_from(row)?);
        }
        Ok(products)
    }

    fn add_known_product(&mut self, product: &NewKnownProduct, at: Timestamp) -> Result<i64, RepoError> {
        let publisher = publisher_text(&product.publisher);
        self.conn
            .execute(
                "INSERT INTO known_products (label, publisher, root, product_name, note, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                 ON CONFLICT (publisher, root) DO UPDATE SET
                    label = excluded.label, note = excluded.note, product_name = excluded.product_name,
                    updated_at = excluded.updated_at",
                params![product.label, publisher, product.root, product.product_name, product.note, at.as_micros()],
            )
            .map_err(backend)?;
        self.conn
            .query_row(
                "SELECT id FROM known_products WHERE publisher = ?1 AND root = ?2",
                params![publisher, product.root],
                |row| row.get(0),
            )
            .map_err(backend)
    }

    fn remove_known_product(&mut self, id: i64) -> Result<bool, RepoError> {
        let removed = self.conn.execute("DELETE FROM known_products WHERE id = ?1", [id]).map_err(backend)?;
        Ok(removed > 0)
    }
}
