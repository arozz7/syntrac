//! A stand-in for `syntrac-service.exe` that tells its version but cannot run, so the installer's rollback can be tried: an upgrade to this program
//! must fail to start the service and put the previous one back. Only used by the installer test in `scripts/release/sandbox`.

use std::process::ExitCode;

use syntrac_service::format_version_report;
use syntrac_store::SqliteStore;

fn main() -> ExitCode {
    if std::env::args().skip(1).any(|argument| argument == "--version") {
        print!("{}", format_version_report(env!("CARGO_PKG_VERSION"), SqliteStore::LATEST_SCHEMA_VERSION));
        return ExitCode::SUCCESS;
    }
    eprintln!("broken stand-in for syntrac-service: refusing to start");
    ExitCode::FAILURE
}
