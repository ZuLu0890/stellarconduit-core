use std::time::Duration;

use crate::message::types::{TopologyFlag, TopologyUpdate};

pub const IDLE_SLEEP_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const SYNCHRONIZED_WAKE_INTERVAL: Duration = Duration::from_secs(5);
pub const SYNCHRONIZED_WAKE_WINDOW: Duration = Duration::from_millis(500);

// Duty-cycle windows (on_ms / period_ms)
const LOW_POWER_BLE_ON_MS: u128 = 200;
const LOW_POWER_BLE_PERIOD_MS: u128 = 10_000;
const DEEP_SLEEP_BLE_ON_MS: u128 = 200;
const DEEP_SLEEP_BLE_PERIOD_MS: u128 = 60_000;

/// Coarse power state for the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Full radio on, gossip rounds at active interval.
    Active,
    /// Duty-cycled BLE (2%), gossip rounds at idle interval.
    LowPower,
    /// Radio off except for periodic wake-scan (0.33% BLE duty).
    DeepSleep,
}

/// Retained for backward compatibility with existing callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    HighPower,
    SynchronizedLowPower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfacePowerState {
    pub ble_enabled: bool,
    pub wifi_enabled: bool,
}

impl InterfacePowerState {
    const fn awake() -> Self {
        Self {
            ble_enabled: true,
            wifi_enabled: true,
        }
    }

    const fn sleeping() -> Self {
        Self {
            ble_enabled: false,
            wifi_enabled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerDecision {
    pub interface_state: InterfacePowerState,
    pub topology_flags: Vec<TopologyFlag>,
    pub wake_network: bool,
}

pub struct PowerManager {
    state: PowerState,
    // Legacy field kept for existing tick() callers
    mode: PowerMode,
    last_incoming_activity_at: Duration,
    pending_outbound_transaction: bool,
    sleep_flag_emitted: bool,
    /// Monotonic "now" in ms, updated by the caller via set_state / tick.
    now_ms: u128,
}

impl PowerManager {
    pub fn new(now: Duration) -> Self {
        Self {
            state: PowerState::Active,
            mode: PowerMode::HighPower,
            last_incoming_activity_at: now,
            pending_outbound_transaction: false,
            sleep_flag_emitted: false,
            now_ms: now.as_millis(),
        }
    }

    // ── New API (Issue-039) ────────────────────────────────────────────────

    /// Recommended gossip round interval for the current power state.
    pub fn recommended_round_interval(&self) -> Duration {
        match self.state {
            PowerState::Active => Duration::from_millis(500),
            PowerState::LowPower => Duration::from_millis(5_000),
            PowerState::DeepSleep => Duration::from_millis(60_000),
        }
    }

    /// Transition to a new power state and return a `TopologyUpdate` that
    /// should be immediately broadcast to peers when entering LowPower or
    /// DeepSleep.
    pub fn set_state(&mut self, state: PowerState) -> Option<TopologyUpdate> {
        self.state = state;
        // Keep legacy mode in sync
        self.mode = match state {
            PowerState::Active => PowerMode::HighPower,
            PowerState::LowPower | PowerState::DeepSleep => PowerMode::SynchronizedLowPower,
        };

        let flags: Vec<TopologyFlag> = match state {
            PowerState::Active => return None,
            PowerState::LowPower => vec![TopologyFlag::LowPowerMode],
            PowerState::DeepSleep => vec![TopologyFlag::DeepSleepPending],
        };

        Some(TopologyUpdate {
            origin_pubkey: [0u8; 32], // caller fills in real pubkey
            directly_connected_peers: vec![],
            hops_to_relay: 0,
            topology_flags: flags,
        })
    }

    /// Duty-cycle gate: returns `true` when BLE advertising should be active
    /// this instant.  Uses a modulo window over `now_ms`.
    pub fn should_advertise_ble(&self) -> bool {
        match self.state {
            PowerState::Active => true,
            PowerState::LowPower => self.now_ms % LOW_POWER_BLE_PERIOD_MS < LOW_POWER_BLE_ON_MS,
            PowerState::DeepSleep => self.now_ms % DEEP_SLEEP_BLE_PERIOD_MS < DEEP_SLEEP_BLE_ON_MS,
        }
    }

    /// Advance the internal clock so duty-cycle logic can be tested without
    /// real wall time.
    pub fn advance_clock(&mut self, now_ms: u128) {
        self.now_ms = now_ms;
    }

    pub fn power_state(&self) -> PowerState {
        self.state
    }

    // ── Legacy API (kept for backward compat) ─────────────────────────────

    pub fn mode(&self) -> PowerMode {
        self.mode
    }

    pub fn interface_state(&self, now: Duration) -> InterfacePowerState {
        if self.mode == PowerMode::HighPower || self.is_awake_window(now) {
            InterfacePowerState::awake()
        } else {
            InterfacePowerState::sleeping()
        }
    }

    pub fn is_awake_window(&self, now: Duration) -> bool {
        let interval_ms = SYNCHRONIZED_WAKE_INTERVAL.as_millis();
        let window_ms = SYNCHRONIZED_WAKE_WINDOW.as_millis();
        now.as_millis() % interval_ms < window_ms
    }

    pub fn next_awake_barrier(&self, now: Duration) -> Duration {
        let interval_ms = SYNCHRONIZED_WAKE_INTERVAL.as_millis();
        let current_ms = now.as_millis();
        let next_ms = ((current_ms / interval_ms) + 1) * interval_ms;
        Duration::from_millis(next_ms as u64)
    }

    pub fn record_incoming_transaction(&mut self, now: Duration) -> PowerDecision {
        self.last_incoming_activity_at = now;
        self.pending_outbound_transaction = false;
        self.sleep_flag_emitted = false;
        self.mode = PowerMode::HighPower;
        self.state = PowerState::Active;
        self.tick(now)
    }

    pub fn record_outbound_transaction(&mut self, now: Duration) -> PowerDecision {
        let mut decision = self.tick(now);

        if self.mode == PowerMode::SynchronizedLowPower && !self.is_awake_window(now) {
            self.pending_outbound_transaction = true;
            decision.interface_state = InterfacePowerState::sleeping();
            return decision;
        }

        self.last_incoming_activity_at = now;
        self.sleep_flag_emitted = false;
        self.mode = PowerMode::HighPower;
        self.state = PowerState::Active;
        decision.interface_state = InterfacePowerState::awake();
        decision
    }

    pub fn tick(&mut self, now: Duration) -> PowerDecision {
        self.now_ms = now.as_millis();
        let mut topology_flags = Vec::new();
        let idle_for = now.saturating_sub(self.last_incoming_activity_at);

        if idle_for >= IDLE_SLEEP_TIMEOUT && self.mode == PowerMode::HighPower {
            self.mode = PowerMode::SynchronizedLowPower;
            self.state = PowerState::LowPower;
        }

        if self.mode == PowerMode::SynchronizedLowPower && !self.sleep_flag_emitted {
            topology_flags.push(TopologyFlag::GoToSleep);
            self.sleep_flag_emitted = true;
        }

        let wake_network = self.mode == PowerMode::SynchronizedLowPower
            && self.pending_outbound_transaction
            && self.is_awake_window(now);

        if wake_network {
            self.mode = PowerMode::HighPower;
            self.state = PowerState::Active;
            self.pending_outbound_transaction = false;
            self.last_incoming_activity_at = now;
            self.sleep_flag_emitted = false;
        }

        PowerDecision {
            interface_state: self.interface_state(now),
            topology_flags,
            wake_network,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Issue-039 required tests ──────────────────────────────────────────

    #[test]
    fn test_power_low_mode_reduces_round_interval() {
        let mut pm = PowerManager::new(Duration::from_secs(0));
        pm.set_state(PowerState::LowPower);
        assert!(pm.recommended_round_interval() >= Duration::from_secs(5));
    }

    #[test]
    fn test_duty_cycle_on_time() {
        // At t=0ms (within first 200ms window) BLE should be on.
        let mut pm = PowerManager::new(Duration::from_secs(0));
        pm.set_state(PowerState::LowPower);
        pm.advance_clock(0);
        assert!(pm.should_advertise_ble());
        // Also true at 199ms
        pm.advance_clock(199);
        assert!(pm.should_advertise_ble());
    }

    #[test]
    fn test_duty_cycle_off_time() {
        // At 500ms into a 10s window BLE should be off.
        let mut pm = PowerManager::new(Duration::from_secs(0));
        pm.set_state(PowerState::LowPower);
        pm.advance_clock(500);
        assert!(!pm.should_advertise_ble());
    }

    #[test]
    fn test_topology_flag_set_on_low_power_transition() {
        let mut pm = PowerManager::new(Duration::from_secs(0));
        let update = pm
            .set_state(PowerState::LowPower)
            .expect("should produce topology update");
        assert!(update.topology_flags.contains(&TopologyFlag::LowPowerMode));
    }

    #[test]
    fn test_topology_flag_deep_sleep_pending() {
        let mut pm = PowerManager::new(Duration::from_secs(0));
        let update = pm
            .set_state(PowerState::DeepSleep)
            .expect("should produce topology update");
        assert!(update
            .topology_flags
            .contains(&TopologyFlag::DeepSleepPending));
    }

    #[test]
    fn test_set_state_active_returns_none() {
        let mut pm = PowerManager::new(Duration::from_secs(0));
        pm.set_state(PowerState::LowPower);
        assert!(pm.set_state(PowerState::Active).is_none());
    }

    #[test]
    fn test_deep_sleep_interval_larger_than_low_power() {
        let mut pm = PowerManager::new(Duration::from_secs(0));
        pm.set_state(PowerState::LowPower);
        let lp = pm.recommended_round_interval();
        pm.set_state(PowerState::DeepSleep);
        let ds = pm.recommended_round_interval();
        assert!(ds > lp);
    }

    // ── Legacy tests (kept) ───────────────────────────────────────────────

    #[test]
    fn awake_window_alignment_uses_shared_modulo_barrier() {
        let manager = PowerManager::new(Duration::from_secs(0));
        assert!(manager.is_awake_window(Duration::from_millis(10)));
        assert!(!manager.is_awake_window(Duration::from_millis(4_999)));
        assert!(manager.is_awake_window(Duration::from_millis(5_000)));
        assert!(manager.is_awake_window(Duration::from_millis(5_499)));
        assert!(!manager.is_awake_window(Duration::from_millis(5_500)));
    }

    #[test]
    fn next_awake_barrier_rounds_up_to_next_five_second_boundary() {
        let manager = PowerManager::new(Duration::from_secs(0));
        assert_eq!(
            manager.next_awake_barrier(Duration::from_millis(1_250)),
            Duration::from_secs(5)
        );
        assert_eq!(
            manager.next_awake_barrier(Duration::from_secs(5)),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn idle_timeout_emits_single_go_to_sleep_flag() {
        let mut manager = PowerManager::new(Duration::from_secs(0));

        let first = manager.tick(IDLE_SLEEP_TIMEOUT);
        assert_eq!(manager.mode(), PowerMode::SynchronizedLowPower);
        assert_eq!(first.topology_flags, vec![TopologyFlag::GoToSleep]);
        assert_eq!(first.interface_state, InterfacePowerState::awake());

        let second = manager.tick(IDLE_SLEEP_TIMEOUT + Duration::from_secs(1));
        assert!(second.topology_flags.is_empty());
        assert_eq!(second.interface_state, InterfacePowerState::sleeping());
    }

    #[test]
    fn sleeping_transaction_waits_for_next_barrier_and_wakes_network() {
        let mut manager = PowerManager::new(Duration::from_secs(0));
        manager.tick(IDLE_SLEEP_TIMEOUT);

        let sleeping =
            manager.record_outbound_transaction(IDLE_SLEEP_TIMEOUT + Duration::from_millis(1_000));
        assert_eq!(sleeping.interface_state, InterfacePowerState::sleeping());
        assert!(!sleeping.wake_network);
        assert_eq!(manager.mode(), PowerMode::SynchronizedLowPower);

        let awake = manager.tick(IDLE_SLEEP_TIMEOUT + Duration::from_secs(5));
        assert!(awake.wake_network);
        assert_eq!(awake.interface_state, InterfacePowerState::awake());
        assert_eq!(manager.mode(), PowerMode::HighPower);
    }
}
