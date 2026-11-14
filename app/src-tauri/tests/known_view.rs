//! What the app shows about the user's known programs: each row's standing against the list, the lookalike list, and the
//! commands that send the write requests to the service.

use std::sync::{Arc, Mutex};

use syntrac_app_lib::backend::Backend;
use syntrac_app_lib::history_programs::{program_history, program_ranking, Grouping, ProgramMetricParam};
use syntrac_app_lib::known_view::known_programs;
use syntrac_app_lib::settings::AppSettings;
use syntrac_core::{
    roll_up_due, Executable, FileFacts, IdentityRecord, IdentityRepository, KnownProductRepository, MetricsRepository,
    NewKnownProduct, ProcessRepository, RollupSettings, Signature, SignatureStatus, Timestamp, VersionInfo,
};
use syntrac_ipc::pipe::{PipeServer, PipeServerConfig};
use syntrac_ipc::protocol::{Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_testkit::fixtures::{at_secs, metrics_sample, process_metric, running_process};
use syntrac_testkit::{InMemoryRepository, ManualClock};

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const TRANSMIT: &str = r"C:\Program Files (x86)\Acme\x64\acmetransmit64.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\chrome.exe";
const FAKE: &str = r"C:\Users\alice\AppData\Local\Temp\acmeserv.exe";
const ACME_ROOT: &str = r"c:\program files (x86)\acme";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
}

fn facts(status: SignatureStatus, signer: Option<&str>, product: Option<&str>) -> FileFacts {
    FileFacts {
        signature: Signature { status, signer: signer.map(str::to_owned), ..Signature::default() },
        version: VersionInfo { product: product.map(str::to_owned), ..VersionInfo::default() },
        ..FileFacts::default()
    }
}

fn record(path: &str, facts: FileFacts) -> IdentityRecord {
    IdentityRecord {
        exe: Executable::from_path(path).unwrap(),
        facts: Some(facts),
        inspected_at: Some(at_secs(BASE)),
        last_attempt: at_secs(BASE),
        failure: None,
    }
}

/// Acme (two programs, signed), Chrome (signed), and an unsigned `acmeserv.exe` in a temp folder; Acme is KNOWN.
fn scenario() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    for (index, path) in [SERV, TRANSMIT, CHROME, FAKE].iter().enumerate() {
        let pid = 10 + index as u32;
        let mut process = running_process(pid, at_secs(secs(0, 0)));
        process.image_path = (*path).to_owned();
        repo.record_start(process.clone()).unwrap();
        repo.append_metrics(&metrics_sample(
            secs(0, 10 + index as i64),
            30.0,
            vec![process_metric(pid, Some(process.id), 1.0 + index as f32)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();
    let acme = || facts(SignatureStatus::Valid, Some("ACME SOFTWARE, INC."), Some("Acme Backup"));
    repo.save_identity(&record(SERV, acme())).unwrap();
    repo.save_identity(&record(TRANSMIT, acme())).unwrap();
    repo.save_identity(&record(CHROME, facts(SignatureStatus::Valid, Some("Google LLC"), Some("Google Chrome"))))
        .unwrap();
    repo.save_identity(&record(FAKE, facts(SignatureStatus::Unsigned, None, None))).unwrap();
    repo.add_known_product(
        &NewKnownProduct {
            label: "Acme".into(),
            publisher: Some("acme software, inc.".into()),
            root: ACME_ROOT.into(),
            product_name: Some("Acme Backup".into()),
            note: Some("my cloud backup".into()),
        },
        at_secs(BASE),
    )
    .unwrap();
    repo
}

fn rows(grouping: Grouping) -> Vec<syntrac_app_lib::history_programs::ProgramRowView> {
    program_ranking(&scenario(), now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 20, grouping).unwrap().programs
}

fn row<'a>(
    rows: &'a [syntrac_app_lib::history_programs::ProgramRowView],
    name: &str,
) -> &'a syntrac_app_lib::history_programs::ProgramRowView {
    rows.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}"))
}

#[test]
fn a_program_of_a_known_product_is_known_and_an_unrelated_one_is_unlisted() {
    let rows = rows(Grouping::None);

    let serv = rows.iter().find(|r| r.path.as_deref() == Some(SERV)).unwrap();
    assert_eq!((serv.standing.state.as_str(), serv.standing.label.as_deref()), ("known", Some("Acme")));
    assert_eq!(row(&rows, "chrome.exe").standing.state, "unlisted");
}

#[test]
fn an_unsigned_program_with_a_known_programs_file_name_in_a_temp_folder_is_flagged_with_its_reasons() {
    let rows = rows(Grouping::None);

    let fake = rows.iter().find(|r| r.path.as_deref() == Some(FAKE)).unwrap();

    assert_eq!(fake.standing.state, "lookalike");
    assert_eq!(fake.standing.label.as_deref(), Some("Acme"));
    assert_eq!(fake.standing.expected_publisher.as_deref(), Some("acme software, inc."));
    assert_eq!(fake.standing.expected_folder.as_deref(), Some(ACME_ROOT));
    assert_eq!(fake.standing.reasons.len(), 1);
    assert!(fake.standing.reasons[0].contains("acmeserv.exe"), "{:?}", fake.standing.reasons);
}

#[test]
fn a_product_row_is_known_when_its_programs_are() {
    let rows = rows(Grouping::ByProduct);

    let product = row(&rows, "Acme Backup");

    assert_eq!(product.members.len(), 2);
    assert_eq!(product.standing.state, "known");
    assert!(product.members.iter().all(|m| m.standing.state == "known"));
}

#[test]
fn a_programs_history_says_where_it_stands() {
    let history = program_history(&scenario(), now(), 7 * 24 * 3600, FAKE).unwrap();

    assert_eq!(history.standing.state, "lookalike");
    let known = program_history(&scenario(), now(), 7 * 24 * 3600, SERV).unwrap();
    assert_eq!(known.standing.state, "known");
}

#[test]
fn the_known_programs_view_lists_each_product_with_its_programs_and_the_lookalikes() {
    let view = known_programs(&scenario()).unwrap();

    assert_eq!(view.products.len(), 1);
    let product = &view.products[0];
    assert_eq!((product.label.as_str(), product.note.as_deref()), ("Acme", Some("my cloud backup")));
    assert_eq!(product.publisher.as_deref(), Some("acme software, inc."));
    assert_eq!(product.folder, ACME_ROOT);
    assert_eq!(product.programs, ["acmeserv.exe", "acmetransmit64.exe"]);
    assert_eq!(view.lookalikes.len(), 1);
    assert_eq!(view.lookalikes[0].path, FAKE);
    assert_eq!(view.lookalikes[0].standing.label.as_deref(), Some("Acme"));
    assert_eq!(view.lookalikes[0].signature.as_deref(), Some("unsigned"));
}

#[test]
fn with_no_known_products_nothing_is_judged() {
    let mut repo = scenario();
    for product in repo.known_products().unwrap() {
        repo.remove_known_product(product.id).unwrap();
    }

    let view = known_programs(&repo).unwrap();

    assert!(view.products.is_empty() && view.lookalikes.is_empty());
    let rows =
        program_ranking(&repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 20, Grouping::None).unwrap().programs;
    assert!(rows.iter().all(|r| r.standing.state == "unlisted"));
}

/// Records what reached it and answers with a fixed reply.
struct Recording {
    seen: Mutex<Vec<Request>>,
    reply: Response,
}

impl Handler for Recording {
    fn handle(&self, request: Request) -> Response {
        self.seen.lock().unwrap().push(request);
        self.reply.clone()
    }
}

fn backend_with_service(reply: Response) -> (Backend<ManualClock>, PipeServer, Arc<Recording>, String) {
    let name = format!("syntrac-test-{}-{}", std::process::id(), BASE + reply_tag(&reply));
    let handler = Arc::new(Recording { seen: Mutex::new(Vec::new()), reply });
    let server = PipeServer::start(PipeServerConfig::new(&name), handler.clone()).expect("pipe starts");
    let settings = AppSettings::for_database(std::path::PathBuf::from("none.db"), &name);
    (Backend::new(settings, ManualClock::new(now())), server, handler, name)
}

fn reply_tag(reply: &Response) -> i64 {
    match reply {
        Response::KnownProductSaved { .. } => 1,
        Response::Done => 2,
        _ => 3,
    }
}

#[test]
fn marking_a_program_sends_the_path_label_and_note_to_the_service() {
    let (backend, _server, handler, _) = backend_with_service(Response::KnownProductSaved { id: 4 });

    let saved = backend.mark_known(SERV.to_owned(), "Acme".into(), Some("my backup".into()));

    assert_eq!(saved, Ok(4));
    assert_eq!(
        handler.seen.lock().unwrap().as_slice(),
        [Request::KnownProductAdd { path: SERV.into(), label: "Acme".into(), note: Some("my backup".into()) }]
    );
}

#[test]
fn a_refusal_from_the_service_reaches_the_user_in_its_own_words() {
    let (backend, _server, _handler, _) =
        backend_with_service(Response::Error { message: "This file has been changed.".into() });

    let saved = backend.mark_known(SERV.to_owned(), "x".into(), None);

    assert_eq!(saved, Err("This file has been changed.".to_owned()));
}

#[test]
fn removing_sends_the_id_and_succeeds_on_done() {
    let (backend, _server, handler, _) = backend_with_service(Response::Done);

    assert_eq!(backend.unmark_known(7), Ok(()));
    assert_eq!(handler.seen.lock().unwrap().as_slice(), [Request::KnownProductRemove { id: 7 }]);
}

#[test]
fn with_the_service_not_running_the_user_is_told_so() {
    let pipe = format!("syntrac-nobody-home-{}", std::process::id());
    let settings = AppSettings::for_database(std::path::PathBuf::from("none.db"), &pipe);
    let backend = Backend::new(settings, ManualClock::new(now()));

    let saved = backend.mark_known(SERV.to_owned(), "x".into(), None);

    let message = saved.unwrap_err();
    assert!(message.to_lowercase().contains("not running"), "{message}");
}
