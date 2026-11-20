# Phase L1: live GPU views (no schema change, still v16; app rebuild only)

Slice L1, requested by the user on 2026-11-20 ("are we doing the live views for gpu as well?"), approved as proposed: a GPU line on the Overview chart and a "Who is using the GPU" card. Built on G1 (`phase-G1.md`) and G2 (`phase-G2.md`).

## What changed
- **Correction.** `phase-G1.md` said the live Overview needed a pipe protocol change. It did not: the app reads the database directly, and `system_series` already returned the GPU reading since G1. L1 needed no service change and no reinstall.
- **Overview chart** (`charts/cpuChart.ts`): a dotted GPU line in its own colour (`palette.gpu`, the theme's `--chart-4`; the history GPU charts use it too now), drawn only when some point has a reading; a point without one is a break in the line, never 0. A "GPU now" fact appears next to CPU and memory when the latest point has a reading.
- **Series** (`views.rs`, `series.rs`): `SystemPointView.gpu_percent: Option<f32>`; the downsampling averages the GPU over the points that HAD a reading only (a point made of none keeps none), so a stretch without counters is neither an idle GPU nor a diluted one.
- **Who is using the GPU** (`GpuContributors.tsx`, `OverviewView.gpu`): programs ranked by their average GPU share over the period (absent samples count as zero), with peak and peak GPU memory, the system's average and peak, and "programs not listed used at most N% each". It says what the figures are a share of (one engine, not the machine), so there is no "unaccounted" row: shares of different engines cannot be added. A period with no GPU reading says the GPU was not recorded; a period with readings and no user says no program used the GPU.
- **New query** `MetricsRepository::gpu_breakdown(from, to, limit)` (`GpuBreakdown`, `GpuContribution` in core): SQL on `process_metrics` / `system_metrics` in the store, plain code in the in-memory repository. Only programs that used the GPU at all are listed (`SUM(gpu_percent) > 0`); the divisor is the samples that had a reading; ties are broken by pid so both stores order alike.
- The name lookup shared by the CPU and GPU cards moved to one helper (`image_path_of`).

## Tests (written first)
- Contract suite (runs on both stores): average over the samples that had a reading, peak and memory peak, window bounds, heaviest first and the limit, a program with no GPU use not listed, the tail's largest single share, a window without readings or without samples.
- App views: names and figures, "no reading is not 0%", the downsampling not diluting the GPU.
- Screens: the chart option (no GPU line without readings, a dotted line with a break where a point had none, its own colour), the card in each state, the "GPU now" fact.

## Not built
- A GPU column on the Processes screen: that screen has no live numbers at all (no CPU or memory column either), so a GPU column would be the first; it belongs with a general "live process metrics" slice if wanted.
