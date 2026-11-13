//! Time primitives. The brain never reads the wall clock directly; it asks a [`Clock`].

use serde::{Deserialize, Serialize};

/// 100 ns ticks between the FILETIME epoch (1601-01-01) and the Unix epoch (1970-01-01).
const FILETIME_TICKS_AT_UNIX_EPOCH: i128 = 116_444_736_000_000_000;
const FILETIME_TICKS_PER_MICRO: i128 = 10;

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

    /// Converts a Windows FILETIME (100 ns ticks since 1601-01-01 UTC).
    ///
    /// This single function is used by every source (ETW and the startup snapshot), so the
    /// same process always yields the same creation time and therefore the same identity.
    pub fn from_filetime(ticks: u64) -> Self {
        let micros = (i128::from(ticks) - FILETIME_TICKS_AT_UNIX_EPOCH) / FILETIME_TICKS_PER_MICRO;
        Self(i64::try_from(micros).unwrap_or(i64::MAX))
    }
}

/// The real wall clock. The ONLY place the system time is read; everything else asks a `Clock`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let micros = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros());
        Timestamp::from_micros(i64::try_from(micros).unwrap_or(i64::MAX))
    }
}

/// Source of "now". Injected so logic is deterministic under test.
pub trait Clock {
    fn now(&self) -> Timestamp;
}
