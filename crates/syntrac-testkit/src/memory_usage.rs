//! `UsageRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::collections::BTreeMap;

use syntrac_core::{ProcessHourRow, RepoError, Timestamp, TrafficHourRow, UsageRepository};

use crate::memory::InMemoryRepository;

impl UsageRepository for InMemoryRepository {
    fn process_hour_rows(&self, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessHourRow>, RepoError> {
        Ok(self
            .rollups
            .processes
            .iter()
            .filter(|((hour, _), _)| *hour >= from && *hour <= to)
            .filter_map(|(_, row)| {
                row.exe.as_ref().map(|exe| ProcessHourRow {
                    exe: exe.clone(),
                    hour: row.hour,
                    cpu_avg: row.cpu_avg,
                    memory_peak: row.memory_peak,
                })
            })
            .collect())
    }

    fn traffic_hour_rows(&self, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficHourRow>, RepoError> {
        let mut sums: BTreeMap<(Timestamp, String), TrafficHourRow> = BTreeMap::new();
        for ((hour, key, addr), row) in &self.rollups.traffic {
            if *hour < from || *hour > to || addr.is_loopback() {
                continue;
            }
            let (Some(exe), Some(key)) = (row.exe.as_ref(), key.as_ref()) else { continue };
            let entry = sums.entry((*hour, key.clone())).or_insert_with(|| TrafficHourRow {
                exe: exe.clone(),
                hour: *hour,
                bytes_sent: 0,
                bytes_received: 0,
            });
            entry.bytes_sent += row.bytes_sent;
            entry.bytes_received += row.bytes_received;
        }
        Ok(sums.into_values().collect())
    }
}
