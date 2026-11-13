//! Time primitives. The brain never reads the wall clock directly; it asks a [`Clock`].

use serde::{Deserialize, Serialize};

/// Microseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn as_micros(self) -> i64 {
        self.0
    }
}

/// Source of "now". Injected so logic is deterministic under test.
pub trait Clock {
    fn now(&self) -> Timestamp;
}
