use std::collections::VecDeque;

/// FIFO ring of recent samples. Push appends; when the buffer is full, the
/// oldest sample is evicted. `drain` returns and clears the contents.
pub struct PreRoll {
    deque: VecDeque<f32>,
    capacity: usize,
}

impl PreRoll {
    pub fn new(capacity_samples: usize) -> Self {
        Self {
            deque: VecDeque::with_capacity(capacity_samples),
            capacity: capacity_samples,
        }
    }

    /// Push samples; oldest evicted if over capacity.
    pub fn push(&mut self, samples: &[f32]) {
        if self.capacity == 0 {
            return;
        }
        for &s in samples {
            if self.deque.len() == self.capacity {
                self.deque.pop_front();
            }
            self.deque.push_back(s);
        }
    }

    /// Drain the current contents into a `Vec<f32>` and clear the ring.
    pub fn drain(&mut self) -> Vec<f32> {
        self.deque.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.deque.len()
    }

    pub fn is_empty(&self) -> bool {
        self.deque.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_below_capacity_retains_all() {
        let mut p = PreRoll::new(10);
        p.push(&[1.0, 2.0, 3.0]);
        assert_eq!(p.len(), 3);
        let drained = p.drain();
        assert_eq!(drained, vec![1.0, 2.0, 3.0]);
        assert!(p.is_empty());
    }

    #[test]
    fn push_above_capacity_evicts_oldest() {
        let mut p = PreRoll::new(3);
        p.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(p.len(), 3);
        let drained = p.drain();
        assert_eq!(drained, vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn drain_clears_buffer() {
        let mut p = PreRoll::new(10);
        p.push(&[1.0, 2.0]);
        let _ = p.drain();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn zero_capacity_never_retains() {
        let mut p = PreRoll::new(0);
        p.push(&[1.0, 2.0, 3.0]);
        assert!(p.is_empty());
        let drained = p.drain();
        assert!(drained.is_empty());
    }
}
