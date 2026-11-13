//! Flow aggregation: per-packet network activity becomes per-window flows, keyed by process INSTANCE.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use syntrac_core::{EventEnvelope, EventPayload, FlowAggregator, NetAction, NetProtocol, ProcessInstanceId, Timestamp};
use uuid::Uuid;

const WINDOW: Duration = Duration::from_secs(10);
const SEC: i64 = 1_000_000;
const REMOTE: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));
const OTHER_REMOTE: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));

fn instance(pid: u32, created_secs: i64) -> ProcessInstanceId {
    ProcessInstanceId::from_pid_and_creation(pid, Timestamp::from_micros(created_secs * SEC))
}

fn activity(
    at_secs: i64,
    owner: Option<ProcessInstanceId>,
    pid: u32,
    action: NetAction,
    remote: IpAddr,
    bytes: u32,
) -> EventEnvelope {
    EventEnvelope {
        id: Uuid::new_v4(),
        ts: Timestamp::from_micros(at_secs * SEC),
        process_instance_id: owner,
        severity: None,
        payload: EventPayload::NetworkActivity {
            pid,
            protocol: NetProtocol::Tcp,
            action,
            remote_addr: remote,
            remote_port: 443,
            bytes,
        },
    }
}

struct FlowView {
    owner: Option<ProcessInstanceId>,
    remote: IpAddr,
    sent: u64,
    received: u64,
    opened: u32,
    closed: u32,
    failed: u32,
    window_start: Timestamp,
    window_end: Timestamp,
}

fn view(flows: &[EventEnvelope]) -> Vec<FlowView> {
    flows
        .iter()
        .map(|event| match &event.payload {
            EventPayload::NetworkFlow {
                remote_addr,
                bytes_sent,
                bytes_received,
                opened,
                closed,
                failed,
                window_start,
                window_end,
                ..
            } => FlowView {
                owner: event.process_instance_id,
                remote: *remote_addr,
                sent: *bytes_sent,
                received: *bytes_received,
                opened: *opened,
                closed: *closed,
                failed: *failed,
                window_start: *window_start,
                window_end: *window_end,
            },
            other => panic!("expected a flow, got {}", other.kind()),
        })
        .collect()
}

#[test]
fn bytes_within_one_window_are_summed_per_direction() {
    let owner = Some(instance(10, 1));
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(2, owner, 10, NetAction::Send, REMOTE, 100));
    aggregator.add(&activity(3, owner, 10, NetAction::Send, REMOTE, 50));
    aggregator.add(&activity(4, owner, 10, NetAction::Receive, REMOTE, 4000));

    let flows = view(&aggregator.flush_all());
    assert_eq!(flows.len(), 1);
    assert_eq!((flows[0].sent, flows[0].received), (150, 4000));
}

#[test]
fn activity_in_different_windows_makes_separate_flows_with_aligned_boundaries() {
    let owner = Some(instance(10, 1));
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(4, owner, 10, NetAction::Send, REMOTE, 1));
    aggregator.add(&activity(34, owner, 10, NetAction::Send, REMOTE, 1));

    let flows = view(&aggregator.flush_all());
    assert_eq!(flows.len(), 2);
    assert_eq!(flows[0].window_start, Timestamp::from_micros(0));
    assert_eq!(flows[0].window_end, Timestamp::from_micros(10 * SEC));
    assert_eq!(flows[1].window_start, Timestamp::from_micros(30 * SEC));
}

#[test]
fn a_reused_pid_never_shares_a_flow_with_the_previous_process() {
    let (first, second) = (Some(instance(10, 1)), Some(instance(10, 5)));
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(2, first, 10, NetAction::Send, REMOTE, 100));
    aggregator.add(&activity(6, second, 10, NetAction::Send, REMOTE, 7));

    let flows = view(&aggregator.flush_all());
    assert_eq!(flows.len(), 2);
    let mut owners: Vec<_> = flows.iter().map(|f| f.owner).collect();
    owners.sort();
    assert_eq!(owners, vec![first.min(second), first.max(second)]);
}

#[test]
fn different_remote_addresses_make_separate_flows() {
    let owner = Some(instance(10, 1));
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(2, owner, 10, NetAction::Send, REMOTE, 1));
    aggregator.add(&activity(2, owner, 10, NetAction::Send, OTHER_REMOTE, 1));

    let flows = view(&aggregator.flush_all());
    let mut remotes: Vec<_> = flows.iter().map(|f| f.remote).collect();
    remotes.sort();
    assert_eq!(remotes, vec![OTHER_REMOTE, REMOTE]);
}

#[test]
fn unattributed_activity_is_grouped_by_pid() {
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(2, None, 777, NetAction::Send, REMOTE, 10));
    aggregator.add(&activity(3, None, 777, NetAction::Send, REMOTE, 20));
    aggregator.add(&activity(3, None, 778, NetAction::Send, REMOTE, 5));

    let flows = view(&aggregator.flush_all());
    assert_eq!(flows.len(), 2);
    assert!(flows.iter().all(|f| f.owner.is_none()));
}

#[test]
fn connects_closes_and_failures_are_counted() {
    let owner = Some(instance(10, 1));
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(1, owner, 10, NetAction::Connect, REMOTE, 0));
    aggregator.add(&activity(2, owner, 10, NetAction::Accept, REMOTE, 0));
    aggregator.add(&activity(3, owner, 10, NetAction::Close, REMOTE, 0));
    aggregator.add(&activity(4, owner, 10, NetAction::Fail, REMOTE, 0));

    let flow = &view(&aggregator.flush_all())[0];
    assert_eq!((flow.opened, flow.closed, flow.failed), (2, 1, 1));
}

#[test]
fn flush_due_only_emits_windows_that_have_ended() {
    let owner = Some(instance(10, 1));
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(4, owner, 10, NetAction::Send, REMOTE, 1)); // window 0..10
    aggregator.add(&activity(14, owner, 10, NetAction::Send, REMOTE, 1)); // window 10..20

    assert!(aggregator.flush_due(Timestamp::from_micros(9 * SEC)).is_empty(), "window still open");
    assert_eq!(aggregator.flush_due(Timestamp::from_micros(10 * SEC)).len(), 1, "first window ended exactly now");
    assert_eq!(aggregator.flush_due(Timestamp::from_micros(10 * SEC)).len(), 0, "already emitted");
    assert_eq!(aggregator.flush_all().len(), 1, "second window flushed at shutdown");
}

#[test]
fn flush_all_empties_the_aggregator() {
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(1, Some(instance(10, 1)), 10, NetAction::Send, REMOTE, 1));
    assert_eq!(aggregator.flush_all().len(), 1);
    assert!(aggregator.flush_all().is_empty());
}

#[test]
fn emitted_flows_carry_the_owner_and_start_of_window_as_their_timestamp() {
    let owner = Some(instance(10, 1));
    let mut aggregator = FlowAggregator::new(WINDOW);
    aggregator.add(&activity(14, owner, 10, NetAction::Send, REMOTE, 1));

    let flows = aggregator.flush_all();
    assert_eq!(flows[0].process_instance_id, owner);
    assert_eq!(flows[0].ts, Timestamp::from_micros(10 * SEC));
}

#[test]
fn events_that_are_not_network_activity_are_ignored() {
    let mut aggregator = FlowAggregator::new(WINDOW);
    let dns = EventEnvelope {
        id: Uuid::new_v4(),
        ts: Timestamp::from_micros(SEC),
        process_instance_id: None,
        severity: None,
        payload: EventPayload::DnsQuery { pid: 1, query: "example.org".into(), addresses: vec![] },
    };
    aggregator.add(&dns);
    assert!(aggregator.flush_all().is_empty());
}
