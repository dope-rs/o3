use crate::marker::ThreadBound;

pub struct FairCredits {
    capacity: usize,
    available: usize,
    protected: usize,
    held: Box<[usize]>,
    reserve_per_lane: usize,
    _thread: ThreadBound,
}

impl FairCredits {
    pub fn new(capacity: usize, lane_count: usize) -> Self {
        assert!(lane_count > 0, "credit lane count must be positive");
        let reserve_per_lane = if lane_count == 1 {
            capacity
        } else {
            capacity / lane_count / 2
        };
        Self::with_reserve(capacity, lane_count, reserve_per_lane)
    }

    pub fn with_reserve(capacity: usize, lane_count: usize, reserve_per_lane: usize) -> Self {
        assert!(lane_count > 0, "credit lane count must be positive");
        let protected = reserve_per_lane
            .checked_mul(lane_count)
            .filter(|&protected| protected <= capacity)
            .expect("credit reserve exceeds capacity");
        Self {
            capacity,
            available: capacity,
            protected,
            held: vec![0; lane_count].into_boxed_slice(),
            reserve_per_lane,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn available(&self) -> usize {
        self.available
    }

    pub fn used(&self) -> usize {
        self.capacity - self.available
    }

    pub fn lane_count(&self) -> usize {
        self.held.len()
    }

    pub fn held_by(&self, lane: usize) -> Option<usize> {
        self.held.get(lane).copied()
    }

    pub fn reserve_per_lane(&self) -> usize {
        self.reserve_per_lane
    }

    pub fn shared_available(&self) -> usize {
        self.available - self.protected
    }

    pub fn can_acquire(&self, lane: usize, amount: usize) -> bool {
        let Some(&held) = self.held.get(lane) else {
            return false;
        };
        if amount > self.available {
            return false;
        }
        let own = self.unclaimed(held).min(amount);
        amount - own <= self.shared_available()
    }

    pub fn try_acquire(&mut self, lane: usize, amount: usize) -> bool {
        if !self.can_acquire(lane, amount) {
            return false;
        }
        self.acquire_unchecked(lane, amount);
        true
    }

    pub fn acquire(&mut self, lane: usize, amount: usize) {
        assert!(
            self.can_acquire(lane, amount),
            "credit acquisition exceeds availability"
        );
        self.acquire_unchecked(lane, amount);
    }

    pub fn release(&mut self, lane: usize, amount: usize) {
        let held = *self.held.get(lane).expect("credit lane out of bounds");
        assert!(held >= amount, "cannot release credits that are not held");
        let next = held - amount;
        self.held[lane] = next;
        self.available += amount;
        self.protected += self.unclaimed(next) - self.unclaimed(held);
    }

    fn acquire_unchecked(&mut self, lane: usize, amount: usize) {
        let held = self.held[lane];
        let next = held + amount;
        self.held[lane] = next;
        self.available -= amount;
        self.protected -= self.unclaimed(held) - self.unclaimed(next);
    }

    fn unclaimed(&self, held: usize) -> usize {
        self.reserve_per_lane.saturating_sub(held)
    }
}
