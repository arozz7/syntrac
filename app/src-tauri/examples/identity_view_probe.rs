//! Rehearsal of the identity views on a COPY of a database that has identities (see `identity_probe` in the service crate):
//! `cargo run --release -p syntrac-app --example identity_view_probe -- <copy.db>`
//!
//! Prints the top programs by network, plain and grouped by product, and one program's identity detail, with timings.

use std::path::PathBuf;
use std::time::Instant;

use syntrac_app_lib::history_programs::{
    program_history, range_totals, rank_range, Grouping, ProgramMetricParam, ProgramRowView,
};
use syntrac_core::{Clock, SystemClock};
use syntrac_store::SqliteStore;

fn line(row: &ProgramRowView) -> String {
    let identity = &row.identity;
    format!(
        "{:<28} {:<10} {:<28} {:<14} sent {:>12} B{}",
        row.name,
        identity.signature.as_deref().unwrap_or(&identity.state),
        identity.publisher.as_deref().unwrap_or("-"),
        identity.location,
        row.bytes_sent,
        if row.members.is_empty() { String::new() } else { format!("  [{} programs]", row.members.len()) }
    )
}

fn main() {
    let path: PathBuf = std::env::args().nth(1).expect("usage: identity_view_probe <copy of the database>").into();
    let store = SqliteStore::open_read_only(&path).expect("open the copy read-only");
    let now = SystemClock.now();
    let week = 7 * 24 * 3600;

    let started = Instant::now();
    let range = range_totals(&store, now, week).expect("sums");
    println!("sums of the range: {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);

    for (title, grouping) in [("not grouped", Grouping::None), ("grouped by product", Grouping::ByProduct)] {
        let started = Instant::now();
        let view = rank_range(&store, &range, ProgramMetricParam::Network, 12, grouping).expect("ranking");
        println!("\n== Top programs by network, {title} ({:.1} ms)", started.elapsed().as_secs_f64() * 1000.0);
        for row in &view.programs {
            println!("{}", line(row));
            for member in &row.members {
                println!(
                    "    - {} ({})",
                    member.name,
                    member.identity.signature.as_deref().unwrap_or(&member.identity.state)
                );
            }
        }
    }

    println!("\n== Grouped, by CPU, 200 rows");
    let started = Instant::now();
    let view = rank_range(&store, &range, ProgramMetricParam::Cpu, 200, Grouping::ByProduct).expect("ranking");
    let products = view.programs.iter().filter(|p| !p.members.is_empty()).count();
    println!(
        "{} rows, {} of them products, {:.1} ms",
        view.programs.len(),
        products,
        started.elapsed().as_secs_f64() * 1000.0
    );

    println!("\n== Identity detail of the busiest program");
    if let Some(path) = view.programs.iter().find_map(|p| p.path.clone()) {
        let history = program_history(&store, now, week, &path).expect("history");
        println!("{}", serde_json::to_string_pretty(&history.identity).unwrap());
    }
}
