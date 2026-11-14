//! `KnownProductRepository` for the in-memory repository: the reference the SQLite one must agree with.

use syntrac_core::{KnownProduct, KnownProductRepository, NewKnownProduct, RepoError, Timestamp};

use crate::memory::InMemoryRepository;

impl KnownProductRepository for InMemoryRepository {
    fn known_products(&self) -> Result<Vec<KnownProduct>, RepoError> {
        let mut list = self.known.clone();
        list.sort_by_key(|product| product.id);
        Ok(list)
    }

    fn add_known_product(&mut self, product: &NewKnownProduct, at: Timestamp) -> Result<i64, RepoError> {
        if let Some(existing) =
            self.known.iter_mut().find(|k| k.publisher == product.publisher && k.root == product.root)
        {
            existing.label.clone_from(&product.label);
            existing.note.clone_from(&product.note);
            existing.product_name.clone_from(&product.product_name);
            return Ok(existing.id);
        }
        self.known_issued += 1;
        self.known.push(KnownProduct {
            id: self.known_issued,
            label: product.label.clone(),
            publisher: product.publisher.clone(),
            root: product.root.clone(),
            product_name: product.product_name.clone(),
            note: product.note.clone(),
            created_at: at,
        });
        Ok(self.known_issued)
    }

    fn remove_known_product(&mut self, id: i64) -> Result<bool, RepoError> {
        let before = self.known.len();
        self.known.retain(|product| product.id != id);
        Ok(self.known.len() != before)
    }
}
