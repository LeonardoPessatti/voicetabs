use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadState {
    Idle,
    Speaking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    RisingEdge { timestamp_ms: u64 },
    FallingEdge { timestamp_ms: u64 },
}

/// VAD hysteresis state machine. One instance per pipeline.
///
/// Defaults match spec §5.2: rising threshold 0.5 sustained 120 ms;
/// falling threshold 0.35 sustained 700 ms. `chunk_ms` is the duration of one
/// VAD prediction (32 ms at 16 kHz with 512-sample chunks).
pub struct VadStateMachine {
    state: VadState,
    rising_threshold: f32,
    falling_threshold: f32,
    rising_required_chunks: u32,
    falling_required_chunks: u32,
    above_count: u32,
    below_count: u32,
}

impl VadStateMachine {
    pub fn new(chunk_ms: u32) -> Self {
        let chunk_ms = chunk_ms.max(1);
        Self {
            state: VadState::Idle,
            rising_threshold: 0.5,
            falling_threshold: 0.35,
            rising_required_chunks: 120 / chunk_ms,
            falling_required_chunks: 700 / chunk_ms,
            above_count: 0,
            below_count: 0,
        }
    }

    pub fn state(&self) -> VadState {
        self.state
    }

    /// Feed one probability + its timestamp. Returns a `VadEvent` if the state
    /// transitioned, `None` otherwise.
    pub fn observe(&mut self, prob: f32, timestamp_ms: u64) -> Option<VadEvent> {
        match self.state {
            VadState::Idle => {
                if prob.partial_cmp(&self.rising_threshold) != Some(Ordering::Less) {
                    self.above_count += 1;
                    if self.above_count >= self.rising_required_chunks {
                        self.state = VadState::Speaking;
                        self.above_count = 0;
                        self.below_count = 0;
                        return Some(VadEvent::RisingEdge { timestamp_ms });
                    }
                } else {
                    self.above_count = 0;
                }
            }
            VadState::Speaking => {
                if prob.partial_cmp(&self.falling_threshold) != Some(Ordering::Greater) {
                    self.below_count += 1;
                    if self.below_count >= self.falling_required_chunks {
                        self.state = VadState::Idle;
                        self.above_count = 0;
                        self.below_count = 0;
                        return Some(VadEvent::FallingEdge { timestamp_ms });
                    }
                } else {
                    self.below_count = 0;
                }
            }
        }
        None
    }

    /// Force the state machine back to Idle and clear counters. Used when the
    /// utterance builder force-closes an utterance at the max-duration cap.
    pub fn force_idle(&mut self) {
        self.state = VadState::Idle;
        self.above_count = 0;
        self.below_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chunk size used by every test: 32 ms (matches Silero at 16 kHz / 512).
    /// At 32 ms, rising needs 4 chunks (120/32 = 3.75 → 3), falling needs
    /// 21 chunks (700/32 = 21.875 → 21).
    const CHUNK_MS: u32 = 32;

    fn observe_n(sm: &mut VadStateMachine, prob: f32, n: u32) -> Vec<VadEvent> {
        let mut events = Vec::new();
        for i in 0..n {
            if let Some(e) = sm.observe(prob, (i as u64) * CHUNK_MS as u64) {
                events.push(e);
            }
        }
        events
    }

    #[test]
    fn idle_stays_idle_on_silence() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        let events = observe_n(&mut sm, 0.05, 1_000);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Idle);
    }

    #[test]
    fn sustained_speech_triggers_rising_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        let events = observe_n(&mut sm, 0.9, 10);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], VadEvent::RisingEdge { .. }));
        assert_eq!(sm.state(), VadState::Speaking);
    }

    #[test]
    fn brief_burst_does_not_trigger_rising_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        // 60 ms of "speech" (2 chunks) — below the 120 ms threshold.
        let events = observe_n(&mut sm, 0.9, 2);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Idle);
    }

    #[test]
    fn rising_then_sustained_silence_triggers_falling_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        // Trigger rising.
        observe_n(&mut sm, 0.9, 10);
        assert_eq!(sm.state(), VadState::Speaking);
        // 800 ms of silence (25 chunks).
        let events = observe_n(&mut sm, 0.05, 25);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], VadEvent::FallingEdge { .. }));
        assert_eq!(sm.state(), VadState::Idle);
    }

    #[test]
    fn brief_pause_does_not_trigger_falling_edge() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        observe_n(&mut sm, 0.9, 10);
        assert_eq!(sm.state(), VadState::Speaking);
        // 300 ms of silence (~9 chunks) — below the 700 ms threshold.
        let events = observe_n(&mut sm, 0.05, 9);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Speaking);
    }

    #[test]
    fn hysteresis_resets_below_count_when_speech_returns() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        observe_n(&mut sm, 0.9, 10);
        observe_n(&mut sm, 0.05, 10); // ~320 ms of silence
        // Re-speech briefly; the below-count must reset.
        observe_n(&mut sm, 0.9, 2);
        // Now another short silence; should NOT immediately fire falling edge
        // because the silence streak restarted from zero.
        let events = observe_n(&mut sm, 0.05, 10);
        assert!(events.is_empty(), "got {events:?}");
        assert_eq!(sm.state(), VadState::Speaking);
    }

    #[test]
    fn force_idle_drops_back_with_no_event() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        observe_n(&mut sm, 0.9, 10);
        assert_eq!(sm.state(), VadState::Speaking);
        sm.force_idle();
        assert_eq!(sm.state(), VadState::Idle);
        // Same hysteresis applies after force_idle.
        let events = observe_n(&mut sm, 0.9, 2);
        assert!(events.is_empty(), "got {events:?}");
    }

    #[test]
    fn timestamp_is_passed_through_to_event() {
        let mut sm = VadStateMachine::new(CHUNK_MS);
        // rising_required_chunks = 120/32 = 3; feed 2 chunks first, then the
        // 3rd chunk (at timestamp 999) should fire the RisingEdge.
        for i in 0..2 {
            assert!(sm.observe(0.9, i * 100).is_none());
        }
        let ev = sm.observe(0.9, 999).expect("rising edge");
        match ev {
            VadEvent::RisingEdge { timestamp_ms } => assert_eq!(timestamp_ms, 999),
            other => panic!("expected RisingEdge, got {other:?}"),
        }
    }
}
