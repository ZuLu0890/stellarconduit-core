use std::collections::HashMap;

use crate::topology::events::{TopologyEvent, TopologyEventBus};

pub struct HopCounter {
    pub peer_distances: HashMap<[u8; 32], u8>,
}

impl Default for HopCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl HopCounter {
    pub fn new() -> Self {
        Self {
            peer_distances: HashMap::new(),
        }
    }

    pub fn update_distance(
        &mut self,
        peer: [u8; 32],
        hops: u8,
        event_bus: Option<&TopologyEventBus>,
    ) {
        let old_hops = self.peer_distances.get(&peer).copied().unwrap_or(255);
        self.peer_distances.insert(peer, hops);

        if let Some(bus) = event_bus {
            if old_hops == 255 && hops < 255 {
                bus.publish(TopologyEvent::RelayReachabilityGained {
                    via_peer: peer,
                    hops,
                });
            }
            if old_hops < 255 && hops == 255 {
                let has_any_finite = self.peer_distances.values().any(|&h| h < 255);
                if !has_any_finite {
                    bus.publish(TopologyEvent::RelayReachabilityLost);
                }
            }
        }
    }

    pub fn local_hop_count(&self, active_connections: &[[u8; 32]]) -> u8 {
        let mut min: Option<u8> = None;
        for p in active_connections.iter() {
            if let Some(&h) = self.peer_distances.get(p) {
                min = Some(match min {
                    Some(m) => m.min(h),
                    None => h,
                });
            }
        }
        match min {
            None => 255,
            Some(255) => 255,
            Some(h) => h.saturating_add(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(b: u8) -> [u8; 32] {
        [b; 32]
    }

    #[test]
    fn returns_unknown_when_no_neighbors_known() {
        let hc = HopCounter::new();
        assert_eq!(hc.local_hop_count(&[pk(2)]), 255);
    }

    #[test]
    fn returns_one_when_neighbor_is_relay() {
        let mut hc = HopCounter::new();
        hc.update_distance(pk(2), 0, None);
        assert_eq!(hc.local_hop_count(&[pk(2)]), 1);
    }

    #[test]
    fn picks_min_plus_one() {
        let mut hc = HopCounter::new();
        hc.update_distance(pk(2), 5, None);
        hc.update_distance(pk(3), 3, None);
        hc.update_distance(pk(4), 7, None);
        assert_eq!(hc.local_hop_count(&[pk(2), pk(3), pk(5)]), 4);
    }
}
