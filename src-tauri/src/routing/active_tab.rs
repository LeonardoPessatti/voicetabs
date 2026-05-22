//! Atomic carrier of the currently-active tab id.
//!
//! This is the entire mechanism behind acceptance criterion A3: the capture
//! worker reads it ONCE at each VAD rising edge, and never re-reads it for
//! the rest of that utterance. Whatever the user does after that (clicking
//! other tabs, opening settings, anything) cannot move the in-flight
//! utterance off its captured destination.
//!
//! The atomic carries `0` as the sentinel for "no tab active"; this matches
//! the fact that SQLite's `INTEGER PRIMARY KEY AUTOINCREMENT` rows start at
//! 1, so a real tab id can never be 0.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct ActiveTab {
    inner: Arc<AtomicI64>,
}

impl ActiveTab {
    pub fn new() -> Self {
        Self { inner: Arc::new(AtomicI64::new(0)) }
    }

    /// Set the active tab id. Pass `0` to clear.
    pub fn set(&self, tab_id: i64) {
        self.inner.store(tab_id, Ordering::Release);
    }

    /// Read the current value. Returns `None` if no tab is active.
    pub fn snapshot(&self) -> Option<i64> {
        match self.inner.load(Ordering::Acquire) {
            0 => None,
            id => Some(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn default_is_none() {
        let a = ActiveTab::new();
        assert_eq!(a.snapshot(), None);
    }

    #[test]
    fn set_then_snapshot_round_trips() {
        let a = ActiveTab::new();
        a.set(42);
        assert_eq!(a.snapshot(), Some(42));
    }

    #[test]
    fn set_zero_clears() {
        let a = ActiveTab::new();
        a.set(42);
        a.set(0);
        assert_eq!(a.snapshot(), None);
    }

    /// The A3-defining test: a value captured BEFORE a later `set` is the
    /// value that survives, regardless of how many writes follow. This is
    /// trivially true because `snapshot()` returns a plain `Option<i64>` —
    /// but the test pins the contract so a future "return a handle" refactor
    /// can't silently break it.
    #[test]
    fn snapshot_is_a_value_not_a_reference() {
        let a = ActiveTab::new();
        a.set(1);
        let captured = a.snapshot();
        // Later writes don't move the captured value.
        a.set(2);
        a.set(3);
        a.set(0);
        a.set(99);
        assert_eq!(captured, Some(1));
    }

    /// Two threads: one rapidly switches tabs, one captures a snapshot
    /// once. Confirms the snapshot is internally consistent (no torn read)
    /// and equals one of the values written. `AtomicI64::load` is
    /// guaranteed-atomic by the standard library — this test exists to
    /// prevent a future refactor from replacing it with something racier.
    #[test]
    fn snapshot_under_concurrent_writes_is_one_of_the_written_values() {
        let a = ActiveTab::new();
        a.set(1);
        let writer_handle = {
            let a = a.clone();
            let barrier = Arc::new(Barrier::new(2));
            let b = barrier.clone();
            let h = thread::spawn(move || {
                b.wait();
                for i in 0..10_000 {
                    a.set((i % 100) + 1);
                }
            });
            barrier.wait();
            h
        };
        let s = a.snapshot();
        writer_handle.join().unwrap();
        let id = s.expect("never None after the initial set(1)");
        assert!(id >= 1 && id <= 100, "torn read? got {id}");
    }
}
