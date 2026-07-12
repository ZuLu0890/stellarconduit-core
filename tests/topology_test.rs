use stellarconduit_core::message::types::TopologyUpdate;
use stellarconduit_core::topology::graph::MeshGraph;
use stellarconduit_core::topology::hop_counter::HopCounter;

fn pk(b: u8) -> [u8; 32] {
    [b; 32]
}

#[test]
fn test_graph_apply_update_stores_edges() {
    let mut graph = MeshGraph::new();
    graph.apply_update(
        &TopologyUpdate {
            origin_pubkey: pk(1),
            directly_connected_peers: vec![pk(2)],
            hops_to_relay: 5,
            topology_flags: vec![],
        },
        None,
    );
    let neighbors = graph.get_neighbors(&pk(1)).expect("node_A should exist");
    assert!(neighbors.contains(&pk(2)));
}

#[test]
fn test_graph_get_neighbors_for_unknown_peer_returns_none() {
    let graph = MeshGraph::new();
    assert!(graph.get_neighbors(&pk(99)).is_none());
}

#[test]
fn test_graph_ignores_self_loop_edges() {
    let mut graph = MeshGraph::new();
    graph.apply_update(
        &TopologyUpdate {
            origin_pubkey: pk(1),
            directly_connected_peers: vec![pk(1), pk(2)],
            hops_to_relay: 5,
            topology_flags: vec![],
        },
        None,
    );
    let neighbors = graph.get_neighbors(&pk(1));
    if let Some(list) = neighbors {
        assert!(!list.contains(&pk(1)), "Self-loops must be ignored");
    }
}

#[test]
fn test_hop_counter_returns_highest_when_no_connections() {
    let hc = HopCounter::new();
    assert_eq!(hc.local_hop_count(&[pk(1)]), 255);
}

#[test]
fn test_hop_counter_calculates_min_plus_one() {
    let mut hc = HopCounter::new();
    hc.update_distance(pk(2), 2, None);
    assert_eq!(hc.local_hop_count(&[pk(2)]), 3);
    hc.update_distance(pk(3), 5, None);
    assert_eq!(hc.local_hop_count(&[pk(2), pk(3)]), 3);
}

#[test]
fn test_hop_counter_recognizes_direct_relay() {
    let mut hc = HopCounter::new();
    hc.update_distance(pk(2), 0, None);
    assert_eq!(hc.local_hop_count(&[pk(2)]), 1);
}
