//! `OwnerRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::net::IpAddr;

use syntrac_core::{lookup_in, DatasetInfo, Owner, OwnerRange, OwnerRepository, RepoError};

use crate::memory::InMemoryRepository;

/// The stored dataset: its description and its ranges (sorted, not overlapping).
#[derive(Debug, Default)]
pub(crate) struct OwnerState {
    info: Option<DatasetInfo>,
    ranges: Vec<OwnerRange>,
}

impl OwnerRepository for InMemoryRepository {
    fn dataset_info(&self) -> Result<Option<DatasetInfo>, RepoError> {
        Ok(self.owners.info.clone())
    }

    fn replace_dataset(&mut self, ranges: &[OwnerRange], info: &DatasetInfo) -> Result<(), RepoError> {
        self.owners = OwnerState { info: Some(info.clone()), ranges: ranges.to_vec() };
        Ok(())
    }

    fn lookup_owner(&self, addr: IpAddr) -> Result<Option<Owner>, RepoError> {
        Ok(lookup_in(&self.owners.ranges, addr).map(Owner::from))
    }
}
