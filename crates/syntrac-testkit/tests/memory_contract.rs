use syntrac_testkit::contract::{event_repository_contract, process_repository_contract};
use syntrac_testkit::InMemoryRepository;

#[test]
fn in_memory_repository_satisfies_the_process_contract() {
    process_repository_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_event_contract() {
    event_repository_contract(InMemoryRepository::default);
}
