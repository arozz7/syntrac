//! Counters for the live ETW adapter, so a run can say what was lost or misread and why.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;

use syntrac_core::Timestamp;

/// An event stamped more than this far ahead of the wall clock counts as a "future" event.
/// Small jitter between the two clocks is normal and ignored.
const FUTURE_TOLERANCE_MICROS: i64 = 1_000_000;

#[derive(Debug, Default)]
pub struct EtwStats {
    received: AtomicU64,
    dropped: AtomicU64,
    /// Keyed by `(provider, event id)` so different failures stay distinguishable.
    parse_errors: Mutex<BTreeMap<(&'static str, u16), u64>>,
    max_lag_micros: AtomicI64,
    max_ahead_micros: AtomicI64,
    future_events: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseErrorCount {
    pub provider: &'static str,
    pub event_id: u16,
    pub count: u64,
}

#[derive(Debug, Clone)]
pub struct EtwReport {
    /// Events delivered to the channel.
    pub received: u64,
    /// Events that did not fit in the channel because the consumer fell behind.
    pub dropped: u64,
    /// Noisiest first.
    pub parse_errors: Vec<ParseErrorCount>,
    pub parse_errors_total: u64,
    /// Largest gap between the wall clock and an event's own timestamp when it was processed.
    pub max_lag_ms: i64,
    /// How far ahead of the wall clock the most-future event was stamped.
    pub max_ahead_ms: i64,
    pub future_events: u64,
}

impl EtwStats {
    pub fn record_received(&self) {
        self.received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_parse_error(&self, provider: &'static str, event_id: u16) {
        if let Ok(mut errors) = self.parse_errors.lock() {
            *errors.entry((provider, event_id)).or_insert(0) += 1;
        }
    }

    /// Compares an event's own timestamp with the wall clock. If ETW timestamps were not system time,
    /// events would show a huge constant lag or sit far in the future.
    pub fn observe_event_time(&self, event_ts: Timestamp, now: Timestamp) {
        let lag = now.as_micros().saturating_sub(event_ts.as_micros());
        if lag >= 0 {
            self.max_lag_micros.fetch_max(lag, Ordering::Relaxed);
            return;
        }
        let ahead = -lag;
        if ahead > FUTURE_TOLERANCE_MICROS {
            self.future_events.fetch_add(1, Ordering::Relaxed);
            self.max_ahead_micros.fetch_max(ahead, Ordering::Relaxed);
        }
    }

    pub fn report(&self) -> EtwReport {
        let mut parse_errors: Vec<ParseErrorCount> = self
            .parse_errors
            .lock()
            .map(|errors| {
                errors
                    .iter()
                    .map(|(&(provider, event_id), &count)| ParseErrorCount { provider, event_id, count })
                    .collect()
            })
            .unwrap_or_default();
        parse_errors
            .sort_by(|a, b| b.count.cmp(&a.count).then(a.provider.cmp(b.provider)).then(a.event_id.cmp(&b.event_id)));

        EtwReport {
            received: self.received.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            parse_errors_total: parse_errors.iter().map(|e| e.count).sum(),
            parse_errors,
            max_lag_ms: self.max_lag_micros.load(Ordering::Relaxed) / 1000,
            max_ahead_ms: self.max_ahead_micros.load(Ordering::Relaxed) / 1000,
            future_events: self.future_events.load(Ordering::Relaxed),
        }
    }
}
