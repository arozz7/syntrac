use syntrac_app_lib::series::downsample;
use syntrac_app_lib::views::SystemPointView;

fn point(ts_ms: i64, cpu: f32, mem: f32, rx: u64) -> SystemPointView {
    SystemPointView {
        ts_ms,
        cpu_percent: cpu,
        memory_percent: mem,
        net_received_bytes: rx,
        net_sent_bytes: 0,
        disk_read_bytes: 0,
        disk_write_bytes: 0,
    }
}

#[test]
fn a_short_series_is_returned_unchanged() {
    let points: Vec<_> = (0..5).map(|i| point(i, i as f32, 1.0, 1)).collect();
    assert_eq!(downsample(points.clone(), 10), points);
}

#[test]
fn a_long_series_is_reduced_to_at_most_the_limit() {
    let points: Vec<_> = (0..1000).map(|i| point(i, 1.0, 1.0, 1)).collect();
    assert!(downsample(points, 100).len() <= 100);
}

#[test]
fn levels_are_averaged_and_per_sample_byte_counts_are_summed_within_a_bucket() {
    let points = vec![
        point(1, 10.0, 40.0, 100),
        point(2, 30.0, 60.0, 200),
        point(3, 50.0, 80.0, 300),
        point(4, 70.0, 100.0, 400),
    ];
    let reduced = downsample(points, 2);
    assert_eq!(reduced.len(), 2);
    assert!((reduced[0].cpu_percent - 20.0).abs() < 1e-3 && (reduced[0].memory_percent - 50.0).abs() < 1e-3);
    assert_eq!(reduced[0].net_received_bytes, 300, "bytes are per-sample deltas, so they add up");
    assert!((reduced[1].cpu_percent - 60.0).abs() < 1e-3);
}

#[test]
fn each_bucket_is_stamped_with_its_last_point_so_the_chart_ends_at_the_newest_data() {
    let points: Vec<_> = (1..=6).map(|i| point(i * 1000, 1.0, 1.0, 1)).collect();
    let reduced = downsample(points, 3);
    assert_eq!(reduced.last().unwrap().ts_ms, 6000);
}

#[test]
fn the_total_bytes_survive_downsampling() {
    let points: Vec<_> = (0..777).map(|i| point(i, 1.0, 1.0, 3)).collect();
    let total: u64 = downsample(points, 50).iter().map(|p| p.net_received_bytes).sum();
    assert_eq!(total, 777 * 3);
}

#[test]
fn an_empty_series_and_a_zero_limit_do_not_panic() {
    assert!(downsample(Vec::new(), 10).is_empty());
    assert!(downsample(vec![point(1, 1.0, 1.0, 1)], 0).len() <= 1);
}
