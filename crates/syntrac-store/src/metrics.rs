//! `MetricsRepository` and `Retention` for SQLite.

use rusqlite::{params, OptionalExtension};
use syntrac_core::{
    CpuBreakdown, CpuContribution, MetricsRepository, MetricsSample, OtherProcesses, PurgeCounts, RepoError, Retention,
    SystemMetrics, SystemPoint, Timestamp,
};

use crate::codec::{instance_from_blob, instance_to_blob, to_i64, to_u64, IdBytes};
use crate::connection::{backend, SqliteStore};

impl MetricsRepository for SqliteStore {
    fn append_metrics(&mut self, sample: &MetricsSample) -> Result<(), RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        let ts = sample.ts.as_micros();

        // A second sample at the same instant replaces the first.
        tx.execute("DELETE FROM process_metrics WHERE ts = ?1", [ts]).map_err(backend)?;
        tx.execute(
            "INSERT OR REPLACE INTO system_metrics
             (ts, cpu_percent, memory_used, memory_total, disk_read, disk_write, net_received, net_sent,
              others_count, others_cpu, others_memory)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                ts,
                sample.system.cpu_percent,
                to_i64(sample.system.memory_used_bytes),
                to_i64(sample.system.memory_total_bytes),
                to_i64(sample.system.disk_read_bytes),
                to_i64(sample.system.disk_write_bytes),
                to_i64(sample.system.net_received_bytes),
                to_i64(sample.system.net_sent_bytes),
                sample.others.process_count,
                sample.others.cpu_percent,
                to_i64(sample.others.memory_bytes),
            ],
        )
        .map_err(backend)?;

        for process in &sample.processes {
            tx.execute(
                "INSERT INTO process_metrics (ts, instance, pid, cpu_percent, memory_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    ts,
                    process.instance.map(instance_to_blob),
                    process.pid,
                    process.cpu_percent,
                    to_i64(process.memory_bytes)
                ],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)
    }

    fn system_series(&self, from: Timestamp, to: Timestamp) -> Result<Vec<SystemPoint>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT ts, cpu_percent, memory_used, memory_total, disk_read, disk_write, net_received, net_sent,
                        others_count, others_cpu, others_memory
                 FROM system_metrics WHERE ts >= ?1 AND ts <= ?2 ORDER BY ts ASC",
            )
            .map_err(backend)?;
        let points = statement
            .query_map(params![from.as_micros(), to.as_micros()], |row| {
                Ok(SystemPoint {
                    ts: Timestamp::from_micros(row.get(0)?),
                    system: SystemMetrics {
                        cpu_percent: row.get(1)?,
                        memory_used_bytes: to_u64(row.get(2)?),
                        memory_total_bytes: to_u64(row.get(3)?),
                        disk_read_bytes: to_u64(row.get(4)?),
                        disk_write_bytes: to_u64(row.get(5)?),
                        net_received_bytes: to_u64(row.get(6)?),
                        net_sent_bytes: to_u64(row.get(7)?),
                    },
                    others: OtherProcesses {
                        process_count: row.get(8)?,
                        cpu_percent: row.get(9)?,
                        memory_bytes: to_u64(row.get(10)?),
                    },
                })
            })
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        Ok(points)
    }

    fn cpu_breakdown(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<CpuBreakdown, RepoError> {
        let (window_samples, system_avg, others_avg): (u32, Option<f64>, Option<f64>) = self
            .conn
            .query_row(
                "SELECT COUNT(*), AVG(cpu_percent), AVG(others_cpu) FROM system_metrics WHERE ts >= ?1 AND ts <= ?2",
                params![from.as_micros(), to.as_micros()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?
            .unwrap_or((0, None, None));

        let mut processes = Vec::new();
        let mut listed_avg = 0.0_f64;
        if window_samples > 0 {
            let divisor = f64::from(window_samples);
            // Summed over ALL listed processes, before LIMIT hides some of them.
            let listed_total: Option<f64> = self
                .conn
                .query_row(
                    "SELECT SUM(cpu_percent) FROM process_metrics WHERE ts >= ?1 AND ts <= ?2",
                    params![from.as_micros(), to.as_micros()],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            listed_avg = listed_total.unwrap_or(0.0) / divisor;

            let mut statement = self
                .conn
                .prepare(
                    "SELECT instance, pid, SUM(cpu_percent), MAX(cpu_percent), COUNT(*)
                     FROM process_metrics WHERE ts >= ?1 AND ts <= ?2
                     GROUP BY instance, pid
                     ORDER BY SUM(cpu_percent) DESC
                     LIMIT ?3",
                )
                .map_err(backend)?;
            let mut rows = statement
                .query(params![from.as_micros(), to.as_micros(), i64::try_from(limit).unwrap_or(i64::MAX)])
                .map_err(backend)?;
            while let Some(row) = rows.next().map_err(backend)? {
                let instance: Option<IdBytes> = row.get(0).map_err(backend)?;
                let sum: f64 = row.get(2).map_err(backend)?;
                let peak: f64 = row.get(3).map_err(backend)?;
                processes.push(CpuContribution {
                    instance: instance.map(instance_from_blob),
                    pid: row.get(1).map_err(backend)?,
                    avg_cpu_percent: (sum / divisor) as f32,
                    peak_cpu_percent: peak as f32,
                    samples_present: row.get(4).map_err(backend)?,
                });
            }
        }

        let system_avg = system_avg.unwrap_or(0.0);
        let others_avg = others_avg.unwrap_or(0.0);
        Ok(CpuBreakdown {
            window_samples,
            system_avg_cpu_percent: system_avg as f32,
            processes,
            others_avg_cpu_percent: others_avg as f32,
            unaccounted_avg_cpu_percent: (system_avg - listed_avg - others_avg).max(0.0) as f32,
        })
    }
}

impl Retention for SqliteStore {
    fn purge_before(&mut self, cutoff: Timestamp) -> Result<PurgeCounts, RepoError> {
        let cutoff = cutoff.as_micros();
        let tx = self.conn.transaction().map_err(backend)?;

        // A flow lives in `flows` under its event's number: delete the flows of the events being purged FIRST, so no
        // flow event can ever be left without its flow row.
        tx.execute("DELETE FROM flows WHERE seq IN (SELECT seq FROM events WHERE ts < ?1)", [cutoff])
            .map_err(backend)?;
        let events = tx.execute("DELETE FROM events WHERE ts < ?1", [cutoff]).map_err(backend)?;
        let processes =
            tx.execute("DELETE FROM processes WHERE ended IS NOT NULL AND ended < ?1", [cutoff]).map_err(backend)?;
        let metric_samples = tx.execute("DELETE FROM system_metrics WHERE ts < ?1", [cutoff]).map_err(backend)?;
        tx.execute("DELETE FROM process_metrics WHERE ts < ?1", [cutoff]).map_err(backend)?;

        tx.commit().map_err(backend)?;
        Ok(PurgeCounts { events: events as u64, processes: processes as u64, metric_samples: metric_samples as u64 })
    }
}
