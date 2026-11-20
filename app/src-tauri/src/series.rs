//! Reducing a long time series to a chart-sized one without distorting it.

use crate::views::SystemPointView;

/// Reduces `points` to at most `max_points` by merging neighbours. Levels (CPU, memory) are averaged; byte counts are
/// per-sample deltas, so they are SUMMED and the totals survive. Each merged point takes the time of its last member,
/// so a chart still ends at the newest data. The GPU level is averaged over the points that HAD a reading only (a point made of
/// none keeps none), so a stretch without GPU counters is neither an idle GPU nor a diluted one.
pub fn downsample(points: Vec<SystemPointView>, max_points: usize) -> Vec<SystemPointView> {
    let max_points = max_points.max(1);
    if points.len() <= max_points {
        return points;
    }
    let bucket = points.len().div_ceil(max_points);
    points
        .chunks(bucket)
        .map(|chunk| {
            let count = chunk.len() as f32;
            SystemPointView {
                ts_ms: chunk.last().map_or(0, |p| p.ts_ms),
                cpu_percent: chunk.iter().map(|p| p.cpu_percent).sum::<f32>() / count,
                memory_percent: chunk.iter().map(|p| p.memory_percent).sum::<f32>() / count,
                net_received_bytes: chunk.iter().map(|p| p.net_received_bytes).sum(),
                net_sent_bytes: chunk.iter().map(|p| p.net_sent_bytes).sum(),
                disk_read_bytes: chunk.iter().map(|p| p.disk_read_bytes).sum(),
                disk_write_bytes: chunk.iter().map(|p| p.disk_write_bytes).sum(),
                gpu_percent: {
                    let readings: Vec<f32> = chunk.iter().filter_map(|p| p.gpu_percent).collect();
                    (!readings.is_empty()).then(|| readings.iter().sum::<f32>() / readings.len() as f32)
                },
            }
        })
        .collect()
}
