//! Rehearsal of known programs on a COPY of a database that has identities (the copy is migrated to the latest schema):
//! `cargo run --release -p syntrac-app --example known_probe -- <copy.db> [path of a program to mark as known ...]`
//!
//! Marks each given program's product as known (the way the service does: from the stored identity), then prints the known products and
//! the programs that look like one. This shows, on real data, whether the lookalike rules stay quiet for the real thing.

use std::path::PathBuf;

use syntrac_app_lib::known_view::known_programs;
use syntrac_core::known::new_known_product;
use syntrac_core::{Clock, IdentityRepository, KnownProductRepository, SystemClock};
use syntrac_store::SqliteStore;

fn main() {
    let mut args = std::env::args().skip(1);
    let path: PathBuf = args.next().expect("usage: known_probe <copy of the database> [program paths]").into();
    let mut store = SqliteStore::open(&path).expect("open the copy (migrates it)");
    println!("schema v{}", store.schema_version().unwrap());

    for program in args {
        let record = store.identity(&program).expect("read identity");
        match new_known_product(&program, record.as_ref(), "Probe label", None) {
            Ok(product) => {
                let id = store.add_known_product(&product, SystemClock.now()).expect("save");
                println!("marked #{id}: publisher {:?}, folder {}", product.publisher, product.root);
            }
            Err(error) => println!("refused {program}: {error}"),
        }
    }

    let view = known_programs(&store).expect("view");
    for product in &view.products {
        println!("\nKNOWN {} ({} programs here)", product.label, product.programs.len());
    }
    println!("\n{} lookalikes", view.lookalikes.len());
    for lookalike in view.lookalikes.iter().take(40) {
        println!(
            "  {:<32} {:<10} {:<14} {}",
            lookalike.name,
            lookalike.signature.as_deref().unwrap_or("-"),
            lookalike.location,
            lookalike.standing.reasons.join("; ")
        );
        println!("      {}", lookalike.path);
    }
}
