# Phase G1: GPU use per program, capture (schema v15 -> v16, additive)

Slice G1 of `docs/ALERTS-PLAN.md`, requested by the user on 2026-11-20 ("should we capture GPU usage and processes as well? There is a chance a rogue app will use GPU compute instead of CPU"). Plan approved as written, with the busiest-engine-across-all-adapters figure and both G1 and G2 built now.

## What changed
- **The unit** (`syntrac-core/src/gpu.rs`, pure, new): `parse_engine_instance`, `parse_memory_instance`, `gpu_figures`, the `GpuSource` port, `GpuReading` / `CounterRow`. A process's figure is the share (0-100) of its BUSIEST ENGINE over all adapters (Task Manager's definition), a share of ONE engine and never divided by a core count; GPU memory is dedicated memory added over adapters. The counter service's `#1` suffix (an instance listed twice) is the same engine and is added up; a row that does not parse is skipped without hiding the rest; negative, NaN and over-100 values are kept inside their range.
- **Windows adapter** (`syntrac-collector/src/gpu_counters.rs`, thin): `PdhGpuSource` reads `\GPU Engine(*)\Utilization Percentage` and `\GPU Process Memory(*)\Dedicated Usage` through PDH. The `windows` crate gets the feature `Win32_System_Performance`; no new package. No counters = `open()` returns `None` and the service logs a warning; GPU is then recorded as "not available", never zero.
- **Sampler** (`sysmetrics.rs`, `metrics_build.rs`): `SysinfoMetricsSource::with_gpu`; `RawProcessUsage` carries GPU figures; `build_sample` lists the top N by CPU PLUS up to 5 more by GPU (at least 1%, heaviest first) so a program heavy on the GPU and light on the CPU is named. The tail keeps `others.gpu_percent` (largest single value) and `others.gpu_memory_bytes` (sum).
- **Types**: `ProcessMetric.{gpu_percent, gpu_memory_bytes}`, `SystemMetrics.gpu_percent: Option<f32>`, `OtherProcesses.{gpu_percent, gpu_memory_bytes}` (all `#[serde(default)]`).
- **Store, migration 16** (additive `ADD COLUMN`s with defaults, no backup): `process_metrics.{gpu_percent, gpu_memory}`, `system_metrics.{gpu_percent (NULL = no reading), others_gpu_percent, others_gpu_memory}`, `rollup_system_hour.{gpu_avg, gpu_max}` (NULL when the hour had no reading), `rollup_process_hour.{gpu_avg, gpu_peak, gpu_memory_peak}`. Old rows keep 0 / NULL.
- **Rollups** (`rollup.rs`): a program's GPU per sample is its instances added and capped at 100, averaged over ALL the hour's samples like the CPU, plus the peak and the memory peak; the system hour's GPU average is over the samples that had a reading only. Trends: `ProcessTotals`/`ProgramSummary` carry `gpu_peak(_percent)` and `gpu_memory_peak(_bytes)`; new ranking `ProgramMetric::GpuPeak` (a program that never used the GPU is not ranked by it); product groups take the highest.
- **Service**: new switch `gpu_metrics_enabled` (default true; passive, local, about 2.4 ms per sample measured on the test machine, about 0.05% of a core at the 5-second cadence, so no reading-every-third-sample fallback was needed). `config/service.toml` documents it.
- **App**: Rust views `SystemHourView.{gpu_avg_percent, gpu_max_percent}` (Option), `ProgramRowView.{gpu_peak_percent, gpu_memory_peak_bytes}`, `ProgramHourView.{gpu_avg_percent, gpu_peak_percent, gpu_memory_peak_bytes}`, metric word `gpu`. Screens: Programs history gets the metric button "GPU", the columns "GPU peak" and "GPU memory peak" (sortable) and a line saying what the figure is a share of; Program detail shows the GPU facts and chart only for a program that used the GPU; System history shows a GPU card only when some hour had a reading.
- **Docs**: `docs/ALERTS-PLAN.md` (slice G and decisions 7, 8), `docs/SCHEMA.md` (v16), `docs/ARCHITECTURE.md` (adapter row), `docs/THREAT-MODEL.md` (GPU row and residuals).
- **Example**: `crates/syntrac-collector/examples/measure_gpu.rs` (read-only; prints timings and counts only).

## Tests (written first)
- Core: `tests/gpu.rs` (12: name parsing, busiest engine, duplicate suffix, adapters, memory, ranges, unparsable rows, empty reading), `tests/rollup.rs` (+5: system average over readings only, no reading is not zero, program average/peak/memory peak, instances capped at one engine, a program that never used the GPU).
- Collector: `tests/metrics_build.rs` (+5: GPU extras listed by name, not listed twice, at most five and not below 1%, listed + tail account for all GPU memory, nothing changes without GPU figures).
- Store: `migration_v16_tests.rs` (a v15 database keeps every row, old rows have no GPU data, new samples store GPU, no backup); the trend contract suite (+2, runs on BOTH the in-memory and SQLite stores): GPU goes raw -> rollup -> range totals -> ranking, and an hour without counters has no GPU figures.
- App views: `history.rs` (+1, plus the GPU fields), `history_programs.rs` (+2).
- Screens and charts: bucketing and chart builders for the program and the system, Programs history, System history.

## Deviations and notes
- The plan said the overview shows a system GPU line. The LIVE overview chart comes from the service pipe (`SystemPointView`), which would need a protocol change; the GPU line is in System history (from the hourly rollups) instead. The live overview is unchanged.
- The top list is the union of the top N by CPU and the top five by GPU (the earlier sketch said "rank by the larger of the two", but the two are not the same unit).
- The service must be reinstalled and the app rebuilt together (new schema, new fields).

## Not built here
- G2 (usage rule for GPU, pattern `sustained_gpu`, backtest section): next slice.
- Per-adapter temperature, power and clock speed: out of scope.
