//! Folding the programs of one product (the four Acme executables) into one row, and ranking the rows.

use syntrac_core::program_group::{group_programs, rank_groups, GroupKey};
use syntrac_core::{Executable, LedgerEntry, ProductKey, ProgramMetric, ProgramSummary, Timestamp};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_micros(n)
}

fn summary(path: &str) -> ProgramSummary {
    ProgramSummary {
        exe: Executable::from_path(path),
        cpu_avg_percent: 0.0,
        cpu_peak_percent: 0.0,
        memory_peak_bytes: 0,
        gpu_peak_percent: 0.0,
        gpu_memory_peak_bytes: 0,
        starts: 0,
        hours_present: 0,
        bytes_sent: 0,
        bytes_received: 0,
        flows: 0,
        destinations: 0,
        ledger: None,
    }
}

fn unattributed() -> ProgramSummary {
    ProgramSummary { exe: None, cpu_avg_percent: 5.0, ..summary("x") }
}

fn product(root: &str) -> Option<ProductKey> {
    Some(ProductKey { publisher: Some("acme software, inc.".into()), root: root.into() })
}

/// Acme programs share a product; everything else has no product key.
fn by_folder(exe: &Executable) -> Option<ProductKey> {
    exe.key.starts_with(r"c:\program files (x86)\acme").then(|| product(r"c:\program files (x86)\acme")).flatten()
}

fn acme() -> Vec<ProgramSummary> {
    vec![
        ProgramSummary {
            cpu_avg_percent: 0.5,
            cpu_peak_percent: 9.0,
            memory_peak_bytes: 300,
            starts: 2,
            hours_present: 10,
            bytes_sent: 1_000,
            bytes_received: 10,
            flows: 5,
            destinations: 3,
            ledger: Some(LedgerEntry { first_seen: ts(10), last_seen: ts(50) }),
            ..summary(r"C:\Program Files (x86)\Acme\acmetransmit64.exe")
        },
        ProgramSummary {
            cpu_avg_percent: 0.25,
            cpu_peak_percent: 20.0,
            memory_peak_bytes: 500,
            starts: 1,
            hours_present: 12,
            bytes_sent: 5,
            bytes_received: 5,
            flows: 1,
            destinations: 1,
            ledger: Some(LedgerEntry { first_seen: ts(5), last_seen: ts(40) }),
            ..summary(r"C:\Program Files (x86)\Acme\acmeserv.exe")
        },
    ]
}

#[test]
fn the_programs_of_one_product_are_added_up_into_one_row() {
    let groups = group_programs(acme(), by_folder);

    assert_eq!(groups.len(), 1);
    let group = &groups[0];
    assert_eq!(group.members.len(), 2);
    let sum = &group.summary;
    assert!((sum.cpu_avg_percent - 0.75).abs() < 1e-9, "averages over the same samples add");
    assert!((sum.cpu_peak_percent - 20.0).abs() < 1e-9, "a peak is the highest peak of any member");
    assert_eq!(sum.memory_peak_bytes, 500, "likewise the largest memory peak of any member");
    assert_eq!((sum.starts, sum.bytes_sent, sum.bytes_received, sum.flows), (3, 1_005, 15, 6));
    assert_eq!(sum.hours_present, 12, "recorded in the hours any member was");
    assert_eq!(sum.destinations, 3, "distinct destinations cannot be added: the largest member's count");
    assert_eq!(
        sum.ledger,
        Some(LedgerEntry { first_seen: ts(5), last_seen: ts(50) }),
        "first of the firsts, last of the lasts"
    );
    assert!(sum.exe.is_none(), "a product row has no single executable");
}

#[test]
fn members_are_listed_in_path_order_so_the_view_never_depends_on_the_order_they_arrived_in() {
    let mut programs = acme();
    programs.reverse();

    let groups = group_programs(programs, by_folder);

    let names: Vec<_> = groups[0].members.iter().map(|m| m.exe.as_ref().unwrap().key.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn programs_without_a_product_stay_on_their_own_rows() {
    let mut programs = acme();
    programs.push(summary(r"C:\Windows\System32\a.exe"));
    programs.push(summary(r"C:\Windows\System32\b.exe"));

    let groups = group_programs(programs, by_folder);

    assert_eq!(groups.len(), 3, "one product row and two single programs");
    assert!(groups.iter().filter(|g| matches!(g.key, GroupKey::Single(_))).all(|g| g.members.len() == 1));
}

#[test]
fn the_unattributed_bucket_is_never_folded_into_anything() {
    let mut programs = acme();
    programs.push(unattributed());

    let groups = group_programs(programs, by_folder);

    let lone: Vec<_> = groups.iter().filter(|g| g.key == GroupKey::Unattributed).collect();
    assert_eq!(lone.len(), 1);
    assert_eq!(lone[0].summary.cpu_avg_percent, 5.0);
}

#[test]
fn ranking_orders_rows_by_the_metric_with_ties_in_a_fixed_order_and_the_unattributed_last() {
    let mut programs = acme(); // product CPU 0.75
    programs.push(ProgramSummary { cpu_avg_percent: 0.75, ..summary(r"C:\Apps\a.exe") });
    programs.push(ProgramSummary { cpu_avg_percent: 2.0, ..summary(r"C:\Apps\z.exe") });
    programs.push(ProgramSummary { cpu_avg_percent: 0.75, ..unattributed() });

    let ranked = rank_groups(group_programs(programs, by_folder), ProgramMetric::CpuAverage, 10);

    let order: Vec<String> = ranked
        .iter()
        .map(|g| match &g.key {
            GroupKey::Product(p) => format!("product {}", p.root),
            GroupKey::Single(key) => key.clone(),
            GroupKey::Unattributed => "unattributed".into(),
        })
        .collect();
    assert_eq!(
        order,
        vec![
            r"c:\apps\z.exe".to_owned(),
            r"c:\apps\a.exe".to_owned(),
            r"product c:\program files (x86)\acme".to_owned(),
            "unattributed".to_owned(),
        ],
        "by value; equal values by key (a single program before a product row is just alphabetical); unattributed last"
    );
}

#[test]
fn rows_with_nothing_for_the_metric_are_left_out_and_the_limit_applies() {
    let mut programs = acme();
    programs.push(ProgramSummary { cpu_avg_percent: 1.0, ..summary(r"C:\Apps\a.exe") });
    programs.push(summary(r"C:\Apps\idle.exe"));

    let ranked = rank_groups(group_programs(programs, by_folder), ProgramMetric::CpuAverage, 1);

    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].key, GroupKey::Single(r"c:\apps\a.exe".into()));
    let all = rank_groups(group_programs(acme(), by_folder), ProgramMetric::Starts, 10);
    assert_eq!(all[0].summary.starts, 3);
}

#[test]
fn a_member_with_nothing_for_the_metric_still_belongs_to_its_product() {
    let mut programs = acme();
    programs[1].bytes_sent = 0;
    programs[1].bytes_received = 0;

    let ranked = rank_groups(group_programs(programs, by_folder), ProgramMetric::NetworkBytes, 10);

    assert_eq!(ranked[0].members.len(), 2, "the idle sibling is listed under the product");
}
