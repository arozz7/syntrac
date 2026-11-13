use syntrac_testkit::contract::{
    event_repository_contract, flow_queries_contract, metrics_repository_contract, process_repository_contract,
    retention_contract,
};
use syntrac_testkit::InMemoryRepository;

#[test]
fn in_memory_repository_satisfies_the_process_contract() {
    process_repository_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_event_contract() {
    event_repository_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_metrics_contract() {
    metrics_repository_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_flow_queries_contract() {
    flow_queries_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_retention_contract() {
    retention_contract(InMemoryRepository::default);
}
