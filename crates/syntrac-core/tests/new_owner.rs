//! The "new network owner" rule (docs/ALERTS-PLAN.md, F4): a program that is not validly signed reaching a network owner it never had
//! before. A validly signed program's new owners are only counted in the digest. All pure.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::findings::Tier;
use syntrac_core::new_owner::{new_owner_candidates, NewOwnerFacts, ProgramOwners, ReachedOwner};
use syntrac_core::{Executable, FileFacts, Owner, Signature, SignatureStatus, Timestamp};

fn at(day: i64) -> Timestamp {
    Timestamp::from_micros(day * 86_400 * 1_000_000)
}

fn addr(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(203, 0, 113, last))
}

fn owner(asn: u32, org: &str) -> Owner {
    Owner { asn, country: "US".into(), org: org.into() }
}

fn unsigned() -> Option<FileFacts> {
    Some(FileFacts {
        signature: Signature { status: SignatureStatus::Unsigned, ..Signature::default() },
        ..FileFacts::default()
    })
}

fn valid() -> Option<FileFacts> {
    Some(FileFacts {
        signature: Signature { status: SignatureStatus::Valid, signer: Some("VENDOR".into()), ..Signature::default() },
        ..FileFacts::default()
    })
}

fn program(path: &str, facts: Option<FileFacts>, earlier: &[u32], new: Vec<ReachedOwner>) -> ProgramOwners {
    ProgramOwners {
        exe: Executable::from_path(path).unwrap(),
        facts,
        is_new: false,
        earlier_owners: earlier.iter().copied().collect::<HashSet<u32>>(),
        reached: new,
    }
}

fn reached(last: u8, asn: u32, org: &str) -> ReachedOwner {
    ReachedOwner { addr: addr(last), owner: owner(asn, org), first_seen: at(12) }
}

fn run(programs: Vec<ProgramOwners>) -> Vec<syntrac_core::Candidate> {
    new_owner_candidates(&NewOwnerFacts { now: at(13), programs: &programs })
}

const TOOL: &str = r"C:\Users\someone\Downloads\tool.exe";

#[test]
fn an_unsigned_program_reaching_an_owner_it_never_had_is_raised_with_the_owner_and_what_it_had_before() {
    let found = run(vec![program(TOOL, unsigned(), &[100, 200], vec![reached(9, 300, "EVIL-NET Example")])]);

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, "new_owner");
    assert_eq!(found[0].subject, format!("{}|as300", TOOL.to_lowercase()));
    assert_eq!(found[0].dedupe_key, format!("new_owner:{}|as300", TOOL.to_lowercase()));
    assert_eq!(found[0].tier, Tier::Suspicious, "unsigned, in a user folder");
    assert!(found[0].evidence.iter().any(|line| line.contains("AS300") && line.contains("EVIL-NET")));
    assert!(found[0].evidence.iter().any(|line| line.contains("203.0.113.9")));
    assert!(found[0].evidence.iter().any(|line| line.contains("2 other")), "{:?}", found[0].evidence);
    assert!(found[0].why.contains("never reached"), "{}", found[0].why);
}

#[test]
fn an_owner_the_program_already_had_is_not_new() {
    assert!(run(vec![program(TOOL, unsigned(), &[300], vec![reached(9, 300, "SAME-NET")])]).is_empty());
}

#[test]
fn a_validly_signed_program_is_left_to_the_digest() {
    assert!(
        run(vec![program(r"C:\Program Files\V\v.exe", valid(), &[100], vec![reached(9, 300, "SOME-CDN")])]).is_empty()
    );
}

#[test]
fn a_program_that_is_itself_new_is_left_to_its_own_finding() {
    let mut new_program = program(TOOL, unsigned(), &[], vec![reached(9, 300, "EVIL-NET")]);
    new_program.is_new = true;

    assert!(run(vec![new_program]).is_empty());
}

#[test]
fn a_program_with_no_earlier_owner_to_compare_with_is_not_judged() {
    assert!(
        run(vec![program(TOOL, unsigned(), &[], vec![reached(9, 300, "EVIL-NET")])]).is_empty(),
        "nothing to differ from"
    );
}

#[test]
fn two_addresses_of_the_same_new_owner_are_one_finding_and_a_second_owner_is_another() {
    let found = run(vec![program(
        TOOL,
        unsigned(),
        &[100],
        vec![reached(9, 300, "EVIL-NET"), reached(10, 300, "EVIL-NET"), reached(11, 400, "OTHER-NET")],
    )]);

    assert_eq!(found.len(), 2);
}

#[test]
fn an_unsigned_program_in_program_files_is_unusual_not_suspicious() {
    let found = run(vec![program(r"C:\Program Files\Small\s.exe", unsigned(), &[100], vec![reached(9, 300, "NET")])]);

    assert_eq!(found[0].tier, Tier::Unusual);
}
