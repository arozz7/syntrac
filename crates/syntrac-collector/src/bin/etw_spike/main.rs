//! ETW feasibility spike (Phase 0). NOT production code.
//!
//! Subscribes to the Kernel-Process, Kernel-Network and DNS-Client providers for N seconds and
//! reports per-provider event counts, event-id mix, parse samples and this process's own CPU use.
//! Kernel providers require an elevated terminal.
//!
//! Usage: `cargo run -p syntrac-collector --bin etw_spike -- [seconds] [process_keyword_hex]`
//! The keyword mask applies to Kernel-Process only (default 10 = process start/stop; 0 = everything).

mod system;

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ferrisetw::parser::Parser;
use ferrisetw::provider::Provider;
use ferrisetw::schema_locator::SchemaLocator;
use ferrisetw::trace::{TraceTrait, UserTrace};
use ferrisetw::EventRecord;

const DEFAULT_SECONDS: u64 = 30;
const MAX_SAMPLES_PER_EVENT_ID: usize = 2;
/// Kernel-Process keyword for process start/stop only (WINEVENT_KEYWORD_PROCESS). 0 = no filtering.
/// Taken from the provider's published manifest, not yet verified on this machine: the report shows whether it works.
const DEFAULT_PROCESS_KEYWORD: u64 = 0x10;

const KERNEL_PROCESS: &str = "22fb2cd6-0e7b-422b-a0c7-2fad1fd0e716";
const KERNEL_NETWORK: &str = "7dd42a49-5329-4832-8dfd-43d979153a88";
const DNS_CLIENT: &str = "1c95126e-7eea-49a9-a3fe-a378b03ddb4d";

#[derive(Default)]
struct ProviderStats {
    total: AtomicU64,
    by_event_id: Mutex<BTreeMap<u16, u64>>,
    /// A few parsed examples per distinct event id, so rare events are not drowned out by noisy ones.
    samples: Mutex<BTreeMap<u16, Vec<String>>>,
}

impl ProviderStats {
    fn record(&self, event_id: u16, describe: impl FnOnce() -> String) {
        self.total.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut ids) = self.by_event_id.lock() {
            *ids.entry(event_id).or_insert(0) += 1;
        }
        if let Ok(mut samples) = self.samples.lock() {
            let bucket = samples.entry(event_id).or_default();
            if bucket.len() < MAX_SAMPLES_PER_EVENT_ID {
                bucket.push(describe());
            }
        }
    }
}

fn describe_process(record: &EventRecord, locator: &SchemaLocator) -> String {
    let Ok(schema) = locator.event_schema(record) else {
        return format!("event {} (schema unavailable)", record.event_id());
    };
    let parser = Parser::create(record, &schema);
    let pid: Result<u32, _> = parser.try_parse("ProcessID");
    let parent: Result<u32, _> = parser.try_parse("ParentProcessID");
    let created: Result<u64, _> = parser.try_parse("CreateTime");
    let image: Result<String, _> = parser.try_parse("ImageName");
    format!(
        "event {} pid={:?} parent_pid={:?} create_time_raw={:?} image={:?}",
        record.event_id(),
        pid.ok(),
        parent.ok(),
        created.ok(),
        image.ok()
    )
}

fn describe_network(record: &EventRecord, locator: &SchemaLocator) -> String {
    let Ok(schema) = locator.event_schema(record) else {
        return format!("event {} (schema unavailable)", record.event_id());
    };
    let parser = Parser::create(record, &schema);
    let pid: Result<u32, _> = parser.try_parse("PID");
    let remote: Result<IpAddr, _> = parser.try_parse("daddr");
    let port: Result<u16, _> = parser.try_parse("dport");
    let port = port.ok();
    format!(
        "event {} pid={:?} daddr={:?} dport_raw={:?} dport_swapped={:?}",
        record.event_id(),
        pid.ok(),
        remote.ok(),
        port,
        port.map(u16::swap_bytes)
    )
}

fn describe_dns(record: &EventRecord, locator: &SchemaLocator) -> String {
    let Ok(schema) = locator.event_schema(record) else {
        return format!("event {} (schema unavailable)", record.event_id());
    };
    let parser = Parser::create(record, &schema);
    let name: Result<String, _> = parser.try_parse("QueryName");
    let results: Result<String, _> = parser.try_parse("QueryResults");
    format!(
        "event {} header_pid={} query={:?} results={:?}",
        record.event_id(),
        record.process_id(),
        name.ok(),
        results.ok()
    )
}

type Describer = fn(&EventRecord, &SchemaLocator) -> String;

fn provider_for(guid: &str, keywords: u64, stats: &Arc<ProviderStats>, describe: Describer) -> Provider {
    let stats = Arc::clone(stats);
    Provider::by_guid(guid)
        .any(keywords)
        .add_callback(move |record: &EventRecord, locator: &SchemaLocator| {
            stats.record(record.event_id(), || describe(record, locator));
        })
        .build()
}

fn print_report(name: &str, stats: &ProviderStats, elapsed: Duration) {
    let total = stats.total.load(Ordering::Relaxed);
    println!("\n[{name}] {total} events ({:.1}/s)", total as f64 / elapsed.as_secs_f64());
    if let Ok(ids) = stats.by_event_id.lock() {
        let mut mix: Vec<_> = ids.iter().collect();
        mix.sort_by(|a, b| b.1.cmp(a.1));
        let top: Vec<String> = mix.iter().take(8).map(|(id, n)| format!("{id}:{n}")).collect();
        println!("  top event ids: {}", top.join("  "));
    }
    if let Ok(samples) = stats.samples.lock() {
        for sample in samples.values().flatten() {
            println!("  sample: {sample}");
        }
    }
}

fn main() {
    let seconds = std::env::args().nth(1).and_then(|arg| arg.parse().ok()).unwrap_or(DEFAULT_SECONDS);
    let process_keywords = std::env::args()
        .nth(2)
        .and_then(|arg| u64::from_str_radix(arg.trim_start_matches("0x"), 16).ok())
        .unwrap_or(DEFAULT_PROCESS_KEYWORD);

    if !system::is_elevated() {
        eprintln!("This spike needs an ELEVATED terminal (Run as administrator). Not elevated: nothing was measured.");
        std::process::exit(2);
    }

    let process_stats = Arc::new(ProviderStats::default());
    let network_stats = Arc::new(ProviderStats::default());
    let dns_stats = Arc::new(ProviderStats::default());

    let trace = UserTrace::new()
        .named(String::from("SyntracSpike"))
        .enable(provider_for(KERNEL_PROCESS, process_keywords, &process_stats, describe_process))
        .enable(provider_for(KERNEL_NETWORK, 0, &network_stats, describe_network))
        .enable(provider_for(DNS_CLIENT, 0, &dns_stats, describe_dns))
        .start();

    let (_trace, handle) = match trace {
        Ok(started) => started,
        Err(error) => {
            eprintln!("Failed to start the ETW trace: {error:?}");
            std::process::exit(1);
        }
    };
    std::thread::spawn(move || {
        let _ = UserTrace::process_from_handle(handle);
    });

    println!("Collecting for {seconds}s, Kernel-Process keyword mask 0x{process_keywords:x}.");
    println!("Use the machine normally AND open and close a few programs (e.g. Notepad, Calculator) so process events occur.");
    let cpu_before = system::own_cpu_time();
    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(seconds));
    let elapsed = started.elapsed();
    let cpu_used = system::own_cpu_time().saturating_sub(cpu_before);

    print_report("Kernel-Process", &process_stats, elapsed);
    print_report("Kernel-Network", &network_stats, elapsed);
    print_report("DNS-Client", &dns_stats, elapsed);

    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let cpu_percent = cpu_used.as_secs_f64() / (elapsed.as_secs_f64() * cores as f64) * 100.0;
    println!(
        "\nConsumer-side CPU (this process only): {:.3}s over {:.1}s on {cores} cores = {cpu_percent:.2}% of total CPU",
        cpu_used.as_secs_f64(),
        elapsed.as_secs_f64()
    );
    println!(
        "Excludes the cost of emitting events in other processes and the kernel, so it understates total overhead."
    );
    println!(
        "Budget target: under 2% idle. If a provider shows 0 events, see aiChangeLog/phase-00.md (troubleshooting)."
    );
}
