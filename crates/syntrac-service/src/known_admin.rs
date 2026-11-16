//! The pipe's write commands: mark a program's product as known, and remove one (`docs/KNOWN-PROGRAMS-DESIGN.md`).
//!
//! Everything a client sends is checked here, and the product itself is built from the identity THIS service measured for the
//! file, never from anything the client claims about its signer. Each change is logged. The read-only commands (ping, status) are
//! left to the status board, which `ServiceHandler` wraps.

use std::sync::{Arc, Mutex};

use syntrac_core::known::new_known_product;
use syntrac_core::{Clock, IdentityRepository, KnownProductRepository};
use syntrac_ipc::protocol::{Request, Response};
use syntrac_ipc::serve::Handler;
use tracing::{info, warn};

use crate::owner_data::OwnerControl;

/// A path longer than this is not a program path (Windows allows 32,767 characters, but the request cap is 4 KiB anyway).
const MAX_PATH_CHARS: usize = 1024;

pub struct KnownProductsAdmin<R, C> {
    repo: Mutex<R>,
    clock: C,
}

fn refuse(message: impl Into<String>) -> Response {
    Response::Error { message: message.into() }
}

impl<R: KnownProductRepository + IdentityRepository, C: Clock> KnownProductsAdmin<R, C> {
    pub fn new(repo: R, clock: C) -> Self {
        Self { repo: Mutex::new(repo), clock }
    }

    /// Runs `work` on the repository (for tests and for reading the list).
    pub fn with_repository<T>(&self, work: impl FnOnce(&R) -> T) -> T {
        let repo = self.repo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        work(&repo)
    }

    pub fn add(&self, path: &str, label: &str, note: Option<&str>) -> Response {
        let path = path.trim();
        if path.is_empty() || path.chars().count() > MAX_PATH_CHARS {
            return refuse(format!("The program's path must be 1 to {MAX_PATH_CHARS} characters."));
        }
        let mut repo = self.repo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let record = match repo.identity(path) {
            Ok(record) => record,
            Err(error) => return refuse(format!("Could not read what is known about that program: {error}")),
        };
        let product = match new_known_product(path, record.as_ref(), label, note) {
            Ok(product) => product,
            Err(error) => return refuse(error.to_string()),
        };
        match repo.add_known_product(&product, self.clock.now()) {
            Ok(id) => {
                info!(id, label = product.label.as_str(), root = product.root.as_str(), "Known product saved");
                Response::KnownProductSaved { id }
            }
            Err(error) => {
                warn!(%error, "Could not save a known product");
                refuse(format!("Could not save it: {error}"))
            }
        }
    }

    pub fn remove(&self, id: i64) -> Response {
        let mut repo = self.repo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match repo.remove_known_product(id) {
            Ok(true) => {
                info!(id, "Known product removed");
                Response::Done
            }
            Ok(false) => refuse("There is no such known program."),
            Err(error) => {
                warn!(%error, "Could not remove a known product");
                refuse(format!("Could not remove it: {error}"))
            }
        }
    }
}

/// The service's request handler: status requests go to the status board, the write commands to the known-products admin.
pub struct ServiceHandler<H, R, C> {
    status: H,
    admin: KnownProductsAdmin<R, C>,
    owner: Option<Arc<dyn OwnerControl>>,
}

impl<H: Handler, R: KnownProductRepository + IdentityRepository, C: Clock> ServiceHandler<H, R, C> {
    pub fn new(status: H, admin: KnownProductsAdmin<R, C>) -> Self {
        Self { status, admin, owner: None }
    }

    /// Adds the owner dataset's commands (status and download). Without it they answer that they are not available.
    #[must_use]
    pub fn with_owner(mut self, owner: Arc<dyn OwnerControl>) -> Self {
        self.owner = Some(owner);
        self
    }

    pub fn admin(&self) -> &KnownProductsAdmin<R, C> {
        &self.admin
    }
}

impl<H, R, C> Handler for ServiceHandler<H, R, C>
where
    H: Handler,
    R: KnownProductRepository + IdentityRepository + Send,
    C: Clock + Send + Sync,
{
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::KnownProductAdd { path, label, note } => self.admin.add(&path, &label, note.as_deref()),
            Request::KnownProductRemove { id } => self.admin.remove(id),
            Request::OwnerDataStatus => match &self.owner {
                Some(owner) => Response::OwnerData(owner.report()),
                None => refuse("Owner data is not available in this service."),
            },
            Request::OwnerDataRefresh => match &self.owner {
                Some(owner) => owner.refresh(),
                None => refuse("Owner data is not available in this service."),
            },
            other => self.status.handle(other),
        }
    }
}
