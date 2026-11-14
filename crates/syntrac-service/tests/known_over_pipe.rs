//! The whole write path for real: a client on a named pipe, the service's handler, and a SQLite file that another connection
//! (the app's, read-only) then reads.

use std::sync::Arc;
use std::time::Duration;

use syntrac_core::{
    FileFacts, IdentityRecord, IdentityRepository, KnownProductRepository, Signature, SignatureStatus, VersionInfo,
};
use syntrac_ipc::pipe::{request, PipeServer, PipeServerConfig};
use syntrac_ipc::protocol::{Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_service::{KnownProductsAdmin, ServiceHandler};
use syntrac_store::SqliteStore;
use syntrac_testkit::identity_contract::seed_executables;
use syntrac_testkit::ManualClock;

const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const TIMEOUT: Duration = Duration::from_secs(5);

struct StatusStub;

impl Handler for StatusStub {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "stub".into() },
            other => Response::Error { message: format!("stub: {other:?}") },
        }
    }
}

fn unique_name(tag: &str) -> String {
    format!("syntrac-known-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], "")
}

#[test]
fn a_program_marked_over_the_pipe_is_stored_in_the_database_and_readable_by_another_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let mut store = SqliteStore::open(&path).unwrap();
    let now = seed_executables(&mut store, &[SERV]);
    store
        .save_identity(&IdentityRecord {
            exe: syntrac_core::Executable::from_path(SERV).unwrap(),
            facts: Some(FileFacts {
                signature: Signature {
                    status: SignatureStatus::Valid,
                    signer: Some("ACME SOFTWARE, INC.".into()),
                    ..Signature::default()
                },
                version: VersionInfo { product: Some("Acme Backup".into()), ..VersionInfo::default() },
                ..FileFacts::default()
            }),
            inspected_at: Some(now),
            last_attempt: now,
            failure: None,
        })
        .unwrap();
    // The service keeps its own connection for the pipe, as the host does.
    let service_side = SqliteStore::open(&path).unwrap();
    let handler =
        Arc::new(ServiceHandler::new(StatusStub, KnownProductsAdmin::new(service_side, ManualClock::new(now))));
    let name = unique_name("add");
    let _server = PipeServer::start(PipeServerConfig::new(&name), handler).expect("pipe starts");

    let saved = request(
        &name,
        &Request::KnownProductAdd { path: SERV.into(), label: "Acme".into(), note: Some("my backup".into()) },
        TIMEOUT,
    )
    .unwrap();

    let Response::KnownProductSaved { id } = saved else { panic!("{saved:?}") };
    let reader = SqliteStore::open_read_only(&path).unwrap();
    let products = reader.known_products().unwrap();
    assert_eq!(products.len(), 1);
    assert_eq!((products[0].id, products[0].label.as_str()), (id, "Acme"));
    assert_eq!(products[0].publisher.as_deref(), Some("acme software, inc."));

    assert_eq!(request(&name, &Request::KnownProductRemove { id }, TIMEOUT).unwrap(), Response::Done);
    assert!(reader.known_products().unwrap().is_empty());
    assert_eq!(request(&name, &Request::Ping, TIMEOUT).unwrap(), Response::Pong { version: "stub".into() });
}

#[test]
fn a_refusal_crosses_the_pipe_in_plain_words_and_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let mut store = SqliteStore::open(&path).unwrap();
    let now = seed_executables(&mut store, &[SERV]);
    let service_side = SqliteStore::open(&path).unwrap();
    let handler =
        Arc::new(ServiceHandler::new(StatusStub, KnownProductsAdmin::new(service_side, ManualClock::new(now))));
    let name = unique_name("refuse");
    let _server = PipeServer::start(PipeServerConfig::new(&name), handler).expect("pipe starts");

    let answer =
        request(&name, &Request::KnownProductAdd { path: SERV.into(), label: "Acme".into(), note: None }, TIMEOUT)
            .unwrap();

    let Response::Error { message } = answer else { panic!("{answer:?}") };
    assert!(message.contains("not been checked yet"), "{message}");
    assert!(store.known_products().unwrap().is_empty());
}
