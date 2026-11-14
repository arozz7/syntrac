use syntrac_testkit::contract::{
    event_repository_contract, flow_queries_contract, metrics_repository_contract, process_queries_contract,
    process_repository_contract, recent_flows_contract, retention_contract,
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

#[test]
fn in_memory_repository_satisfies_the_process_queries_contract() {
    process_queries_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_recent_flows_contract() {
    recent_flows_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_rollup_contract() {
    syntrac_testkit::rollup_contract::rollup_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_storage_stats_contract() {
    syntrac_testkit::storage_contract::storage_stats_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_trend_contract() {
    syntrac_testkit::trend_contract::trend_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_identity_contract() {
    syntrac_testkit::identity_contract::identity_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_known_products_contract() {
    syntrac_testkit::known_contract::known_contract(InMemoryRepository::default);
}
