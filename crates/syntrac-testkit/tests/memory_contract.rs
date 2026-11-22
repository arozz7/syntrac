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
fn in_memory_repository_satisfies_the_names_contract() {
    syntrac_testkit::names_contract::names_contract(InMemoryRepository::default);
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

#[test]
fn in_memory_repository_satisfies_the_spawn_contract() {
    syntrac_testkit::spawn_contract::spawn_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_destination_contract() {
    syntrac_testkit::destination_contract::destination_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_context_contract() {
    syntrac_testkit::context_contract::context_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_owner_contract() {
    syntrac_testkit::owner_contract::owner_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_launcher_contract() {
    syntrac_testkit::launcher_contract::launcher_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_install_contract() {
    syntrac_testkit::install_contract::install_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_findings_contract() {
    syntrac_testkit::findings_contract::findings_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_identity_change_contract() {
    syntrac_testkit::identity_contract::identity_change_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_usage_contract() {
    syntrac_testkit::usage_contract::usage_contract(InMemoryRepository::default);
}

#[test]
fn in_memory_repository_satisfies_the_process_window_contract() {
    syntrac_testkit::process_window_contract::process_window_contract(InMemoryRepository::default);
}
