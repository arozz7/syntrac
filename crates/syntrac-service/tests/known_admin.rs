//! The service's answers to the pipe's write commands (mark a program as known, remove one): what it accepts, what it refuses,
//! and that the old read-only commands still work through the same handler.

use syntrac_core::{
    FileFacts, IdentityRecord, IdentityRepository, KnownProductRepository, Signature, SignatureStatus, Timestamp,
    VersionInfo,
};
use syntrac_ipc::protocol::{Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_service::{KnownProductsAdmin, ServiceHandler};
use syntrac_testkit::identity_contract::seed_executables;
use syntrac_testkit::{InMemoryRepository, ManualClock};

const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const UNREAD: &str = r"C:\Program Files\Other\unread.exe";
const GONE: &str = r"C:\Program Files\Other\gone.exe";
const CHANGED: &str = r"C:\Program Files\Other\changed.exe";

fn facts(status: SignatureStatus, signer: &str) -> FileFacts {
    FileFacts {
        signature: Signature { status, signer: Some(signer.to_owned()), ..Signature::default() },
        version: VersionInfo { product: Some("Acme Backup".into()), ..VersionInfo::default() },
        ..FileFacts::default()
    }
}

fn record(path: &str, facts: Option<FileFacts>) -> IdentityRecord {
    IdentityRecord {
        exe: syntrac_core::Executable::from_path(path).unwrap(),
        facts,
        inspected_at: Some(Timestamp::from_micros(1)),
        last_attempt: Timestamp::from_micros(1),
        failure: None,
    }
}

/// A service whose database knows five programs: one read and signed, one not yet read, one whose file was gone, one altered.
fn handler() -> ServiceHandler<StatusStub, InMemoryRepository, ManualClock> {
    let mut repo = InMemoryRepository::default();
    let now = seed_executables(&mut repo, &[SERV, UNREAD, GONE, CHANGED]);
    repo.save_identity(&record(SERV, Some(facts(SignatureStatus::Valid, "ACME SOFTWARE, INC.")))).unwrap();
    let mut gone = record(GONE, None);
    gone.failure = Some(syntrac_core::IdentityFailure {
        kind: syntrac_core::InspectErrorKind::NotFound,
        message: "no such file".into(),
        attempts: 1,
    });
    repo.save_identity(&gone).unwrap();
    repo.save_identity(&record(CHANGED, Some(facts(SignatureStatus::Altered, "Someone")))).unwrap();
    ServiceHandler::new(StatusStub, KnownProductsAdmin::new(repo, ManualClock::new(now)))
}

/// Stands in for the status board.
struct StatusStub;

impl Handler for StatusStub {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "stub".into() },
            other => Response::Error { message: format!("status stub: {other:?}") },
        }
    }
}

fn add(handler: &impl Handler, path: &str, label: &str, note: Option<&str>) -> Response {
    handler.handle(Request::KnownProductAdd { path: path.into(), label: label.into(), note: note.map(str::to_owned) })
}

fn error_text(response: Response) -> String {
    match response {
        Response::Error { message } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn the_read_only_commands_still_go_to_the_status_board() {
    assert_eq!(handler().handle(Request::Ping), Response::Pong { version: "stub".into() });
}

#[test]
fn a_program_that_has_been_read_can_be_marked_known_and_the_product_comes_from_the_measured_identity() {
    let handler = handler();

    let response = add(&handler, SERV, "  Acme ", Some("my cloud backup"));

    let Response::KnownProductSaved { id } = response else { panic!("{response:?}") };
    let products = handler.admin().with_repository(|repo| repo.known_products().unwrap());
    assert_eq!(products.len(), 1);
    let product = &products[0];
    assert_eq!(product.id, id);
    assert_eq!(product.label, "Acme");
    assert_eq!(product.publisher.as_deref(), Some("acme software, inc."), "taken from the file's own signature");
    assert_eq!(product.root, r"c:\program files (x86)\acme");
    assert_eq!(product.note.as_deref(), Some("my cloud backup"));
}

#[test]
fn marking_the_same_product_again_updates_it() {
    let handler = handler();
    let Response::KnownProductSaved { id: first } = add(&handler, SERV, "Acme", None) else { panic!() };

    let Response::KnownProductSaved { id: second } = add(&handler, SERV, "Cloud backup", Some("renamed")) else {
        panic!()
    };

    assert_eq!(first, second);
    let products = handler.admin().with_repository(|repo| repo.known_products().unwrap());
    assert_eq!((products.len(), products[0].label.as_str()), (1, "Cloud backup"));
}

#[test]
fn a_program_whose_file_has_not_been_read_yet_is_refused_with_a_reason() {
    let message = error_text(add(&handler(), UNREAD, "Other", None));

    assert!(message.contains("not been checked yet"), "{message}");
}

#[test]
fn a_program_whose_file_could_not_be_read_is_refused_with_a_reason() {
    let message = error_text(add(&handler(), GONE, "Other", None));

    assert!(message.contains("could not be read"), "{message}");
}

#[test]
fn a_file_changed_since_it_was_signed_is_refused() {
    let message = error_text(add(&handler(), CHANGED, "Other", None));

    assert!(message.contains("changed since it was signed"), "{message}");
}

#[test]
fn a_path_the_service_has_never_seen_is_refused_like_one_not_yet_read() {
    let message = error_text(add(&handler(), r"C:\Nowhere\ghost.exe", "Ghost", None));

    assert!(message.contains("not been checked yet"), "{message}");
}

#[test]
fn the_path_is_matched_without_regard_to_case() {
    let response = add(&handler(), &SERV.to_uppercase(), "Acme", None);

    assert!(matches!(response, Response::KnownProductSaved { .. }), "{response:?}");
}

#[test]
fn bad_labels_notes_and_paths_are_refused_and_nothing_is_stored() {
    let handler = handler();
    let long_path = format!(r"C:\{}\a.exe", "x".repeat(1100));

    for (path, label, note) in [
        (SERV, "", None),
        (SERV, "two\nlines", None),
        (SERV, &"x".repeat(81)[..], None),
        (SERV, "ok", Some("bad\u{7}note")),
        (SERV, "ok", Some(&"n".repeat(501)[..])),
        (&long_path[..], "ok", None),
        ("", "ok", None),
        ("   ", "ok", None),
    ] {
        assert!(matches!(add(&handler, path, label, note), Response::Error { .. }), "{path:?} {label:?}");
    }
    assert!(handler.admin().with_repository(|repo| repo.known_products().unwrap()).is_empty());
}

#[test]
fn a_known_product_can_be_removed_and_removing_it_twice_is_an_error_not_a_crash() {
    let handler = handler();
    let Response::KnownProductSaved { id } = add(&handler, SERV, "Acme", None) else { panic!() };

    assert_eq!(handler.handle(Request::KnownProductRemove { id }), Response::Done);
    let again = error_text(handler.handle(Request::KnownProductRemove { id }));

    assert!(again.contains("no such"), "{again}");
    assert!(handler.admin().with_repository(|repo| repo.known_products().unwrap()).is_empty());
}

#[test]
fn a_storage_failure_is_reported_not_swallowed() {
    // A repository that cannot save: the handler must say so rather than claim success.
    struct Failing(InMemoryRepository);
    impl IdentityRepository for Failing {
        fn identity_entries(&self) -> Result<Vec<syntrac_core::IdentityEntry>, syntrac_core::RepoError> {
            self.0.identity_entries()
        }
        fn save_identity(&mut self, r: &IdentityRecord) -> Result<(), syntrac_core::RepoError> {
            self.0.save_identity(r)
        }
        fn identity(&self, path: &str) -> Result<Option<IdentityRecord>, syntrac_core::RepoError> {
            self.0.identity(path)
        }
    }
    impl KnownProductRepository for Failing {
        fn known_products(&self) -> Result<Vec<syntrac_core::KnownProduct>, syntrac_core::RepoError> {
            self.0.known_products()
        }
        fn add_known_product(
            &mut self,
            _: &syntrac_core::NewKnownProduct,
            _: Timestamp,
        ) -> Result<i64, syntrac_core::RepoError> {
            Err(syntrac_core::RepoError::Backend("disk full".into()))
        }
        fn remove_known_product(&mut self, id: i64) -> Result<bool, syntrac_core::RepoError> {
            self.0.remove_known_product(id)
        }
    }
    let mut inner = InMemoryRepository::default();
    let now = seed_executables(&mut inner, &[SERV]);
    inner.save_identity(&record(SERV, Some(facts(SignatureStatus::Valid, "ACME SOFTWARE, INC.")))).unwrap();
    let handler = ServiceHandler::new(StatusStub, KnownProductsAdmin::new(Failing(inner), ManualClock::new(now)));

    let message = error_text(add(&handler, SERV, "Acme", None));

    assert!(message.contains("disk full"), "{message}");
}
