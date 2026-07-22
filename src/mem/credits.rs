use std::array;

use crate::marker::ThreadBound;

/// Fair accounting for one or more resources shared by fixed lanes.
///
/// Acquisitions preserve every lane's unclaimed reserve. Multi-resource
/// operations update every dimension or leave the accounting unchanged.
pub struct FairCredits<const N: usize = 1> {
    capacity: [usize; N],
    available: [usize; N],
    protected: [usize; N],
    held: Box<[[usize; N]]>,
    reserve: [usize; N],
    _thread: ThreadBound,
}

impl<const N: usize> FairCredits<N> {
    /// Builds independent dimensions using the default balanced reserve.
    pub fn from_capacities(capacity: [usize; N], lane_count: usize) -> Self {
        assert!(N > 0, "credit dimension count must be positive");
        assert!(lane_count > 0, "credit lane count must be positive");
        let reserve = capacity.map(|amount| {
            if lane_count == 1 {
                amount
            } else {
                amount / lane_count / 2
            }
        });
        Self::with_reserve_per_lane(capacity, lane_count, reserve)
    }

    /// Builds dimensions with the same reserve assigned to every lane.
    pub fn with_reserve_per_lane(
        capacity: [usize; N],
        lane_count: usize,
        reserve_per_lane: [usize; N],
    ) -> Self {
        assert!(N > 0, "credit dimension count must be positive");
        assert!(lane_count > 0, "credit lane count must be positive");
        let protected = array::from_fn(|dimension| {
            assert!(
                reserve_per_lane[dimension] <= capacity[dimension] / lane_count,
                "credit reserve exceeds capacity"
            );
            reserve_per_lane[dimension] * lane_count
        });
        Self {
            capacity,
            available: capacity,
            protected,
            held: vec![[0; N]; lane_count].into_boxed_slice(),
            reserve: reserve_per_lane,
            _thread: ThreadBound::NEW,
        }
    }

    pub const fn capacities(&self) -> [usize; N] {
        self.capacity
    }

    pub const fn available_all(&self) -> [usize; N] {
        self.available
    }

    pub fn used_all(&self) -> [usize; N] {
        array::from_fn(|dimension| self.capacity[dimension] - self.available[dimension])
    }

    pub fn lane_count(&self) -> usize {
        self.held.len()
    }

    pub fn held_all(&self, lane: usize) -> Option<[usize; N]> {
        self.held.get(lane).copied()
    }

    pub fn shared_available_all(&self) -> [usize; N] {
        array::from_fn(|dimension| self.available[dimension] - self.protected[dimension])
    }

    pub fn can_acquire_all(&self, lane: usize, amount: [usize; N]) -> bool {
        let Some(held) = self.held.get(lane) else {
            return false;
        };
        for dimension in 0..N {
            if amount[dimension] > self.available[dimension] {
                return false;
            }
            let own = self
                .unclaimed(dimension, held[dimension])
                .min(amount[dimension]);
            if amount[dimension] - own > self.available[dimension] - self.protected[dimension] {
                return false;
            }
        }
        true
    }

    /// Atomically checks and acquires every resource dimension.
    pub fn try_acquire_all(&mut self, lane: usize, amount: [usize; N]) -> bool {
        let Some(held) = self.held.get(lane).copied() else {
            return false;
        };
        let mut own = [0; N];
        for dimension in 0..N {
            if amount[dimension] > self.available[dimension] {
                return false;
            }
            own[dimension] = self
                .unclaimed(dimension, held[dimension])
                .min(amount[dimension]);
            if amount[dimension] - own[dimension]
                > self.available[dimension] - self.protected[dimension]
            {
                return false;
            }
        }
        for dimension in 0..N {
            self.held[lane][dimension] = held[dimension] + amount[dimension];
            self.available[dimension] -= amount[dimension];
            self.protected[dimension] -= own[dimension];
        }
        true
    }

    /// Releases every resource dimension as one state transition.
    pub fn release_all(&mut self, lane: usize, amount: [usize; N]) {
        let held = self.held[lane];
        for dimension in 0..N {
            assert!(
                held[dimension] >= amount[dimension],
                "cannot release credits that are not held"
            );
        }
        for dimension in 0..N {
            let next = held[dimension] - amount[dimension];
            let before = self.unclaimed(dimension, held[dimension]);
            let after = self.unclaimed(dimension, next);
            self.held[lane][dimension] = next;
            self.available[dimension] += amount[dimension];
            self.protected[dimension] += after - before;
        }
    }

    fn unclaimed(&self, dimension: usize, held: usize) -> usize {
        self.reserve[dimension].saturating_sub(held)
    }
}

impl FairCredits {
    pub fn new(capacity: usize, lane_count: usize) -> Self {
        Self::from_capacities([capacity], lane_count)
    }

    pub fn with_reserve(capacity: usize, lane_count: usize, reserve_per_lane: usize) -> Self {
        Self::with_reserve_per_lane([capacity], lane_count, [reserve_per_lane])
    }

    pub const fn capacity(&self) -> usize {
        self.capacity[0]
    }

    pub const fn available(&self) -> usize {
        self.available[0]
    }

    pub fn used(&self) -> usize {
        self.capacity[0] - self.available[0]
    }

    pub fn held_by(&self, lane: usize) -> Option<usize> {
        self.held.get(lane).map(|held| held[0])
    }

    pub fn reserved_for(&self, lane: usize) -> Option<usize> {
        self.held.get(lane).map(|_| self.reserve[0])
    }

    pub fn shared_available(&self) -> usize {
        self.available[0] - self.protected[0]
    }

    #[inline]
    pub fn can_acquire(&self, lane: usize, amount: usize) -> bool {
        self.can_acquire_all(lane, [amount])
    }

    #[inline]
    pub fn try_acquire(&mut self, lane: usize, amount: usize) -> bool {
        self.try_acquire_all(lane, [amount])
    }

    #[inline]
    pub fn release(&mut self, lane: usize, amount: usize) {
        self.release_all(lane, [amount]);
    }
}
