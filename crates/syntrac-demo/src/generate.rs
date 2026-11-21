//! Builds the demo database from nothing (docs/PUBLIC-PLAN.md, P1), through the REAL repositories, the REAL rollup code and the REAL
//! detectors, so the screens show what the product really does with the fictional machine of `world`. Sixteen days of history, ending at
//! `now`: raw samples, processes, flows and DNS answers, the hourly rollups, what the files are, how programs start, what is installed,
//! two known products, the owners of the addresses, and the findings the rules raise.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::Duration;

use syntrac_core::findings::ReconcileSettings;
use syntrac_core::{
    addr_key, built_in_pack, cpu_floor_for_cores, roll_up_due, DatasetInfo, DetectionStateRepository, Detector,
    EventRepository, Executable, ExpectationRepository, FileFacts, FileStamp, FindingRepository, IdentityRecord,
    IdentityRepository, InstallRecord, InstallRepository, KnownProductRepository, LauncherKind, LauncherObservation,
    LauncherRepository, MetricsRepository, NewExpectation, NewKnownProduct, NewOwnerDetector, NewThingsDetector,
    OwnerRange, OwnerRepository, PatternDetector, ProcessInstanceId, ProcessRecord, ProcessRepository, RollupSettings,
    Signature, SignatureKind, SignatureStatus, Timestamp, UsageDetector, UsageSettings, VersionInfo,
};
use syntrac_service::{DetectTick, DetectWorker, DetectWorkerSettings};
use syntrac_store::SqliteStore;
use syntrac_testkit::fixtures::{dns_query, net_flow, process_started, EventFactory};
use syntrac_testkit::ManualClock;

use crate::world::{
    instance, is_busy, sample_at, slot_traffic, Moment, DAY_SECS, DESTINATIONS, HOUR_SECS, OWNERS, PROGRAMS,
};

/// What was built, for the command line to print.
#[derive(Debug, Default)]
pub struct Summary {
    pub samples: usize,
    pub processes: usize,
    pub events: usize,
    pub findings: Vec<(String, String)>,
}

fn micros(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * 1_000_000)
}

fn failed(what: &str) -> impl Fn(syntrac_core::RepoError) -> String + '_ {
    move |error| format!("{what}: {error}")
}

/// A 64-digit hex text that looks like a SHA-256 and is made from the path (nothing is hashed from a real file).
fn fake_sha(path: &str) -> String {
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    let mut out = String::new();
    for round in 0..4_u64 {
        for byte in path.bytes().chain(round.to_le_bytes()) {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0100_0000_01b3);
        }
        out.push_str(&format!("{state:016x}"));
    }
    out
}

fn file_name(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

fn folder_of(path: &str) -> String {
    path.rsplit_once('\\').map_or_else(String::new, |(folder, _)| folder.to_lowercase())
}

/// A process of one program on one day, as the samples show it.
struct Seen {
    index: usize,
    id: ProcessInstanceId,
    pid: u32,
    start: i64,
    last: i64,
}

pub fn generate(path: &Path, now: i64) -> Result<Summary, String> {
    if path.exists() {
        return Err(format!(
            "{} already exists; the demo database is made from nothing, so remove it first.",
            path.display()
        ));
    }
    let moment = Moment::new(now);
    let mut store = SqliteStore::open(path).map_err(failed("Could not create the database"))?;
    let mut summary = Summary::default();

    // 1. The past: samples, the processes they came from, flows and DNS answers.
    let mut instances: BTreeMap<(usize, i64), Seen> = BTreeMap::new();
    let mut ts = moment.history_start();
    while ts <= now {
        let step = if ts < now - 2 * HOUR_SECS { 60 } else { 10 };
        for (index, program) in PROGRAMS.iter().enumerate() {
            if is_busy(program, &moment, ts) {
                let (id, pid, start) = instance(index, &moment, ts);
                let seen = instances.entry((index, ts.div_euclid(DAY_SECS))).or_insert(Seen {
                    index,
                    id,
                    pid,
                    start,
                    last: ts,
                });
                seen.last = ts + step;
            }
        }
        store.append_metrics(&sample_at(&moment, ts, step)).map_err(failed("Could not store a sample"))?;
        summary.samples += 1;
        ts += step;
    }
    for seen in instances.values() {
        let program = &PROGRAMS[seen.index];
        let mut record = ProcessRecord {
            id: seen.id,
            pid: seen.pid,
            parent_pid: 4,
            parent_id: None,
            image_path: program.path.to_owned(),
            command_line: None,
            started: micros(seen.start),
            ended: None,
            ended_approximate: false,
            exit_code: None,
        };
        store.record_start(record.clone()).map_err(failed("Could not store a process"))?;
        if seen.last < now - 300 {
            record.ended = Some(micros(seen.last));
            store.record_exit(seen.id, micros(seen.last), Some(0)).map_err(failed("Could not store a process end"))?;
        }
        summary.processes += 1;
    }

    let mut factory = EventFactory::default();
    let mut events = Vec::new();
    for seen in instances.values() {
        let program = &PROGRAMS[seen.index];
        let mut started = factory.make(micros(seen.start), process_started(seen.pid, 4, seen.start, program.path));
        started.process_instance_id = Some(seen.id);
        events.push(started);
    }
    let mut asked: BTreeMap<(usize, i64), ()> = BTreeMap::new();
    let mut slot = moment.history_start().div_euclid(600) * 600;
    while slot <= now {
        for index in 0..PROGRAMS.len() {
            let flows = slot_traffic(index, &moment, slot);
            if flows.is_empty() {
                continue;
            }
            let (id, pid, _) = instance(index, &moment, slot);
            for (dest, sent, received) in flows {
                let [a, b, c, d] = DESTINATIONS[dest].addr;
                let mut flow =
                    factory.make(micros(slot), net_flow(pid, Ipv4Addr::new(a, b, c, d), sent, received, slot));
                flow.process_instance_id = Some(id);
                events.push(flow);
                if asked.insert((dest, slot.div_euclid(DAY_SECS)), ()).is_none() {
                    let answer = dns_query(pid, DESTINATIONS[dest].name, &[Ipv4Addr::new(a, b, c, d)]);
                    let mut query = factory.make(micros(slot), answer);
                    query.process_instance_id = Some(id);
                    events.push(query);
                }
            }
        }
        slot += 600;
    }
    for batch in events.chunks(4_000) {
        store.append(batch).map_err(failed("Could not store events"))?;
    }
    summary.events = events.len();

    // 2. The hourly rollups, exactly as the service makes them.
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 10_000 };
    roll_up_due(&mut store, micros(now), &settings).map_err(failed("Could not roll up the hours"))?;

    // 3. What the files are, how programs start, what is installed, what the user marked known, who owns the addresses.
    for (index, program) in PROGRAMS.iter().enumerate() {
        let first_seen = now - program.first_seen_hours_ago * HOUR_SECS;
        let facts = FileFacts {
            stamp: FileStamp {
                size: 400_000 + 53_000 * index as u64,
                modified: Some(micros(first_seen - 3 * DAY_SECS)),
            },
            sha256: Some(fake_sha(program.path)),
            signature: Signature {
                status: if program.signed { SignatureStatus::Valid } else { SignatureStatus::Unsigned },
                signer: program.signed.then(|| program.company.to_owned()),
                thumbprint: None,
                kind: program.signed.then_some(SignatureKind::Embedded),
            },
            version: VersionInfo {
                company: (!program.company.is_empty()).then(|| program.company.to_owned()),
                product: (!program.product.is_empty()).then(|| program.product.to_owned()),
                description: (!program.description.is_empty()).then(|| program.description.to_owned()),
                original_filename: Some(file_name(program.path).to_owned()),
                file_version: (!program.product.is_empty()).then(|| format!("{}.{}.0", 2 + index % 5, index % 9)),
            },
            zone_id: program.downloaded.then_some(3),
        };
        let exe = Executable::from_path(program.path).ok_or("a program path of the demo cannot be read")?;
        store
            .save_identity(&IdentityRecord {
                exe,
                facts: Some(facts),
                inspected_at: Some(micros(now - HOUR_SECS)),
                last_attempt: micros(now - HOUR_SECS),
                failure: None,
            })
            .map_err(failed("Could not store a file identity"))?;
    }
    record_launchers(&mut store, now)?;
    record_installs_and_known(&mut store, now)?;
    let ranges: Vec<OwnerRange> = OWNERS
        .iter()
        .zip(["US", "DE", "DE", "NL", "US", "GB"])
        .map(|((asn, start, end, org), country)| OwnerRange {
            start: addr_key(IpAddr::V4(Ipv4Addr::from(*start))),
            end: addr_key(IpAddr::V4(Ipv4Addr::from(*end))),
            asn: *asn,
            country: country.to_owned(),
            org: (*org).to_owned(),
        })
        .collect();
    store
        .replace_dataset(
            &ranges,
            &DatasetInfo {
                source: "demo data (invented networks)".to_owned(),
                downloaded_at: micros(now - 3 * DAY_SECS),
                rows: u32::try_from(ranges.len()).unwrap_or(0),
                bad_lines: 0,
                sha256: "0".repeat(64),
            },
        )
        .map_err(failed("Could not store the owner data"))?;

    // 4. Findings: the user said one program is expected; detection was switched on six days ago; the real rules run once.
    store
        .add_expectation(
            &NewExpectation {
                kind: "pattern_unsigned_program_keeps_gpu_busy".to_owned(),
                subject: PROGRAMS[8].path.to_lowercase(),
                note: Some("Runs on purpose: my local AI server.".to_owned()),
                source_finding: None,
                ceiling: None,
            },
            micros(now - DAY_SECS),
        )
        .map_err(failed("Could not store an expectation"))?;
    store.switch_on(micros(moment.detection_switched_on())).map_err(failed("Could not switch detection on"))?;
    let mut worker = DetectWorker::new(
        store,
        detectors(path)?,
        ManualClock::new(micros(now)),
        DetectWorkerSettings { interval: Duration::from_secs(300), reconcile: ReconcileSettings { max_new: 50 } },
    );
    match worker.tick() {
        DetectTick::Ran(_) => {}
        other => return Err(format!("The rules did not run: {other:?}")),
    }
    let found = worker.repository().findings(None, 500).map_err(failed("Could not read the findings"))?;
    summary.findings = found.into_iter().map(|finding| (finding.kind, finding.tier.as_str().to_owned())).collect();
    Ok(summary)
}

/// The rules the service runs (the new-thing, new-owner, usage and pattern rules), each on its own connection.
fn detectors(path: &Path) -> Result<Vec<Box<dyn Detector + Send>>, String> {
    let open = || SqliteStore::open(path).map_err(failed("Could not open the database"));
    let usage = UsageSettings { cpu_floor_percent: cpu_floor_for_cores(8), ..UsageSettings::default() };
    Ok(vec![
        Box::new(NewThingsDetector::new(open()?)),
        Box::new(NewOwnerDetector::new(open()?)),
        Box::new(UsageDetector::new(open()?, usage)),
        Box::new(PatternDetector::new(open()?, built_in_pack())),
    ])
}

fn launcher(path: &str, kind: LauncherKind, location: &str, arguments: &str) -> Result<LauncherObservation, String> {
    Ok(LauncherObservation {
        exe: Executable::from_path(path).ok_or("a program path of the demo cannot be read")?,
        kind,
        location: location.to_owned(),
        detail: String::new(),
        arguments: arguments.to_owned(),
    })
}

/// How programs start: a service and a Run entry that were here from the beginning, and a Run entry of the unsigned program that
/// appeared two days ago (after detection was switched on, so the "new start entry" rule has something to say).
fn record_launchers(store: &mut SqliteStore, now: i64) -> Result<(), String> {
    let run_key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let entries = [
        (PROGRAMS[4].path, LauncherKind::ServiceAuto, "AcmeBackupService", "--service", now - 16 * DAY_SECS),
        (
            PROGRAMS[2].path,
            LauncherKind::RunKey,
            &format!(r"{run_key}\NorthwindSync"),
            "--minimised",
            now - 16 * DAY_SECS,
        ),
        (PROGRAMS[8].path, LauncherKind::RunKey, &format!(r"{run_key}\LocalAI"), "--serve", now - 12 * DAY_SECS),
        (PROGRAMS[9].path, LauncherKind::RunKey, &format!(r"{run_key}\toolx"), "--quiet", now - 2 * DAY_SECS + 3_600),
    ];
    for (path, kind, location, arguments, at) in entries {
        let row = launcher(path, kind, location, arguments)?;
        store.record_launchers(&[row], &[], micros(at)).map_err(failed("Could not store a start entry"))?;
    }
    Ok(())
}

fn record_installs_and_known(store: &mut SqliteStore, now: i64) -> Result<(), String> {
    let products = [
        ("Fabrikam Browser", "Fabrikam, Inc.", "125.0.1", PROGRAMS[0].path),
        ("Contoso Editor", "Contoso Ltd", "3.8.2", PROGRAMS[1].path),
        ("Northwind Sync", "Northwind Traders", "4.2.0", PROGRAMS[2].path),
        ("Acme Backup", "Acme Software, Inc.", "9.1.4", PROGRAMS[4].path),
        ("Tailspin Racing", "Tailspin Toys", "1.7", PROGRAMS[7].path),
    ];
    let rows: Vec<InstallRecord> = products
        .iter()
        .map(|(name, publisher, version, path)| InstallRecord {
            hive: "HKLM".to_owned(),
            key_name: (*name).to_owned(),
            display_name: (*name).to_owned(),
            publisher: Some((*publisher).to_owned()),
            version: Some((*version).to_owned()),
            install_date: Some("20260201".to_owned()),
            location: Some(folder_of(path)),
            derived: false,
            system_component: false,
        })
        .collect();
    store
        .record_installs(&rows, micros(now - 15 * DAY_SECS))
        .map_err(failed("Could not store the installed products"))?;
    for (label, publisher, path) in [
        ("Acme Backup", "acme software, inc.", PROGRAMS[4].path),
        ("Northwind Sync", "northwind traders", PROGRAMS[2].path),
    ] {
        store
            .add_known_product(
                &NewKnownProduct {
                    label: label.to_owned(),
                    publisher: Some(publisher.to_owned()),
                    root: folder_of(path),
                    product_name: Some(label.to_owned()),
                    note: Some("Installed on purpose.".to_owned()),
                },
                micros(now - 10 * DAY_SECS),
            )
            .map_err(failed("Could not store a known product"))?;
    }
    Ok(())
}
