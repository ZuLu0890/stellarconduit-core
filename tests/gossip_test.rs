use std::collections::HashSet;
use std::time::Duration;

use stellarconduit_core::gossip::bloom::SlidingBloomFilter;
use stellarconduit_core::gossip::fanout::{select_random_peers, FanoutCalculator};
use stellarconduit_core::gossip::round::GossipScheduler;
use stellarconduit_core::peer::identity::PeerIdentity;

fn msg_id(byte: u8) -> [u8; 32] {
    [byte; 32]
}

// ==========================================
// Bloom Filter Tests
// ==========================================

#[test]
fn test_bloom_new_message_returns_false() {
    let mut filter = SlidingBloomFilter::new(1000, 0.01);
    assert!(!filter.check_and_add(&msg_id(1)));
}

#[test]
fn test_bloom_seen_message_returns_true() {
    let mut filter = SlidingBloomFilter::new(1000, 0.01);
    filter.check_and_add(&msg_id(1));
    assert!(filter.check_and_add(&msg_id(1)));
}

#[test]
fn test_bloom_rotates_on_capacity() {
    let mut filter = SlidingBloomFilter::new(10, 0.01);

    for i in 0u32..10 {
        let mut id = [0u8; 32];
        id[0..4].copy_from_slice(&i.to_le_bytes());
        filter.check_and_add(&id);
    }

    // Insert one more to trigger rotation
    let trigger = [10u8; 32];
    filter.check_and_add(&trigger);

    // After rotation, previously inserted items should still be found (in previous window)
    let mut first = [0u8; 32];
    first[0] = 0;
    assert!(filter.check_and_add(&first));
}

#[test]
fn test_bloom_false_positive_rate_acceptable() {
    let mut filter = SlidingBloomFilter::new(10_000, 0.01);
    let capacity = 10_000;

    for i in 0u32..capacity as u32 {
        let mut id = [0u8; 32];
        id[0..4].copy_from_slice(&i.to_le_bytes());
        filter.check_and_add(&id);
    }

    let mut false_positives = 0;
    let test_sample_size = 1000;

    for i in 0u32..test_sample_size as u32 {
        let offset = (capacity as u32) + i;
        let mut id = [0u8; 32];
        id[0..4].copy_from_slice(&offset.to_le_bytes());
        if filter.check_and_add(&id) {
            false_positives += 1;
        }
    }

    let fp_rate = (false_positives as f64) / (test_sample_size as f64);
    assert!(
        fp_rate <= 0.05,
        "False positive rate too high: {:.2}%",
        fp_rate * 100.0
    );
}

// ==========================================
// Round Scheduler Tests
// ==========================================

#[test]
fn test_scheduler_triggers_in_active_mode() {
    let scheduler = GossipScheduler::new();
    let interval = scheduler.current_interval();
    assert!(
        interval <= Duration::from_millis(500),
        "Active interval should be fast"
    );
}

#[test]
fn test_scheduler_downgrades_to_idle() {
    let mut scheduler = GossipScheduler::new();
    scheduler.last_active_msg_time = std::time::Instant::now() - Duration::from_secs(60);
    assert!(scheduler.is_idle());
    let interval = scheduler.current_interval();
    assert!(
        interval >= Duration::from_secs(5),
        "Idle interval should be slow"
    );
}

// ==========================================
// Fanout Calculator Tests
// ==========================================

#[test]
fn test_fanout_below_min_returns_all_connections() {
    let calc = FanoutCalculator::new();
    assert_eq!(calc.calculate(1, None), 1);
    assert_eq!(calc.calculate(2, None), 2);
}

#[test]
fn test_fanout_above_max_capped() {
    let calc = FanoutCalculator::new();
    let target = calc.calculate(100, None);
    assert!(
        target <= 6,
        "Fanout should be capped at MAX_FANOUT. Got: {}",
        target
    );
}

#[test]
fn test_select_random_peers_unique() {
    let peers: Vec<PeerIdentity> = (0u8..5).map(|i| PeerIdentity::new([i; 32])).collect();

    let selected = select_random_peers(&peers, 3);
    assert_eq!(selected.len(), 3);

    let unique_peers: HashSet<PeerIdentity> = selected.into_iter().collect();
    assert_eq!(
        unique_peers.len(),
        3,
        "Selected peers must be strictly unique"
    );
}
