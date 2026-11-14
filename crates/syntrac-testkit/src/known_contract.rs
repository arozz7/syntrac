//! Contract suite for known products: every `KnownProductRepository` (in-memory, SQLite, ...) must pass these.

use syntrac_core::{KnownProductRepository, NewKnownProduct, Retention, Timestamp};

pub trait KnownSubject: KnownProductRepository + Retention {}
impl<T> KnownSubject for T where T: KnownProductRepository + Retention {}

pub fn known_contract<R: KnownSubject>(make: impl Fn() -> R) {
    an_empty_list_is_empty(make());
    a_product_is_stored_with_everything_given(make());
    adding_the_same_product_again_updates_it_and_keeps_its_id_and_date(make());
    a_product_without_a_publisher_is_distinct_from_one_with(make());
    removing_returns_whether_anything_was_removed_and_ids_are_not_reused(make());
    products_are_listed_in_the_order_they_were_added(make());
    known_products_survive_the_purge_of_detail(make());
}

fn at(n: i64) -> Timestamp {
    Timestamp::from_micros(n)
}

fn product(label: &str, publisher: Option<&str>, root: &str) -> NewKnownProduct {
    NewKnownProduct {
        label: label.to_owned(),
        publisher: publisher.map(str::to_owned),
        root: root.to_owned(),
        product_name: Some("Acme Backup".into()),
        note: Some("my backup".into()),
    }
}

const ACME: &str = r"c:\program files (x86)\acme";

fn an_empty_list_is_empty<R: KnownSubject>(repo: R) {
    assert!(repo.known_products().unwrap().is_empty());
}

fn a_product_is_stored_with_everything_given<R: KnownSubject>(mut repo: R) {
    let id = repo.add_known_product(&product("Acme", Some("acme software, inc."), ACME), at(100)).unwrap();

    let list = repo.known_products().unwrap();

    assert_eq!(list.len(), 1);
    let stored = &list[0];
    assert_eq!(stored.id, id);
    assert_eq!(stored.label, "Acme");
    assert_eq!((stored.publisher.as_deref(), stored.root.as_str()), (Some("acme software, inc."), ACME));
    assert_eq!(stored.product_name.as_deref(), Some("Acme Backup"));
    assert_eq!(stored.note.as_deref(), Some("my backup"));
    assert_eq!(stored.created_at, at(100));
}

fn adding_the_same_product_again_updates_it_and_keeps_its_id_and_date<R: KnownSubject>(mut repo: R) {
    let first = repo.add_known_product(&product("Acme", Some("acme software, inc."), ACME), at(100)).unwrap();
    let mut renamed = product("Cloud backup", Some("acme software, inc."), ACME);
    renamed.note = None;

    let second = repo.add_known_product(&renamed, at(200)).unwrap();

    assert_eq!(first, second);
    let list = repo.known_products().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].label.as_str(), list[0].note.as_deref()), ("Cloud backup", None));
    assert_eq!(list[0].created_at, at(100), "the date it was first confirmed does not move");
}

fn a_product_without_a_publisher_is_distinct_from_one_with<R: KnownSubject>(mut repo: R) {
    let with = repo.add_known_product(&product("A", Some("acme"), r"c:\apps\x"), at(1)).unwrap();
    let without = repo.add_known_product(&product("B", None, r"c:\apps\x"), at(2)).unwrap();
    let again = repo.add_known_product(&product("B2", None, r"c:\apps\x"), at(3)).unwrap();

    assert_ne!(with, without);
    assert_eq!(without, again, "no publisher is one value, not a wildcard");
    assert_eq!(repo.known_products().unwrap().len(), 2);
}

fn removing_returns_whether_anything_was_removed_and_ids_are_not_reused<R: KnownSubject>(mut repo: R) {
    let first = repo.add_known_product(&product("A", Some("a"), r"c:\a"), at(1)).unwrap();

    assert!(repo.remove_known_product(first).unwrap());
    assert!(!repo.remove_known_product(first).unwrap(), "already gone");
    assert!(!repo.remove_known_product(9_999).unwrap());
    let second = repo.add_known_product(&product("B", Some("b"), r"c:\b"), at(2)).unwrap();
    assert_ne!(first, second, "an id that was removed is not handed out again");
}

fn products_are_listed_in_the_order_they_were_added<R: KnownSubject>(mut repo: R) {
    for (n, label) in ["one", "two", "three"].iter().enumerate() {
        repo.add_known_product(&product(label, Some(label), &format!(r"c:\{label}")), at(n as i64)).unwrap();
    }

    let labels: Vec<String> = repo.known_products().unwrap().into_iter().map(|p| p.label).collect();

    assert_eq!(labels, ["one", "two", "three"]);
}

fn known_products_survive_the_purge_of_detail<R: KnownSubject>(mut repo: R) {
    repo.add_known_product(&product("A", Some("a"), r"c:\a"), at(1)).unwrap();
    let before = repo.known_products().unwrap();

    repo.purge_before(at(1_000_000_000)).unwrap();

    assert_eq!(repo.known_products().unwrap(), before);
}
