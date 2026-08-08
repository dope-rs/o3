use std::{array, cell};

/// Shared surplus for independently stored fair-lane states.
pub struct Pool<const N: usize = 1> {
    shared: cell::Cell<[usize; N]>,
    _thread: crate::ThreadBound,
}

/// One lane's protected reserve and current holdings.
pub struct State<const N: usize = 1> {
    reserve: [usize; N],
    held: cell::Cell<[usize; N]>,
}

/// A borrowed lane view that updates all resource dimensions atomically.
#[derive(Clone, Copy)]
pub struct Lane<'a, const N: usize = 1> {
    shared: &'a cell::Cell<[usize; N]>,
    held: &'a cell::Cell<[usize; N]>,
    reserve: [usize; N],
}

/// Owned fair accounting with one uniform protected reserve per lane.
pub struct Credits<const N: usize = 1> {
    used: cell::Cell<[usize; N]>,
    pool: Pool<N>,
    held: Box<[cell::Cell<[usize; N]>]>,
    reserve: [usize; N],
}

impl<const N: usize> Pool<N> {
    pub fn new(shared: [usize; N]) -> Self {
        use crate::ThreadBound;
        assert!(N > 0, "credit dimension count must be positive");
        Self {
            shared: cell::Cell::new(shared),
            _thread: ThreadBound::NEW,
        }
    }

    pub fn bind<'a>(&'a self, state: &'a State<N>) -> Lane<'a, N> {
        Lane {
            shared: &self.shared,
            held: &state.held,
            reserve: state.reserve,
        }
    }

    fn shared(&self) -> [usize; N] {
        self.shared.get()
    }
}

impl<const N: usize> State<N> {
    pub fn new(reserve: [usize; N]) -> Self {
        Self {
            reserve,
            held: cell::Cell::new([0; N]),
        }
    }

    pub fn split_at(total: [usize; N], lane_count: usize, lane: usize) -> Self {
        assert!(lane_count > 0, "credit lane count must be positive");
        assert!(lane < lane_count, "credit lane out of bounds");
        Self::new(array::from_fn(|dimension| {
            total[dimension] / lane_count + usize::from(lane < total[dimension] % lane_count)
        }))
    }
}

impl<const N: usize> Lane<'_, N> {
    fn can_acquire_all(self, amount: [usize; N]) -> bool {
        let held = self.held.get();
        let shared = self.shared.get();
        for dimension in 0..N {
            if held[dimension].checked_add(amount[dimension]).is_none() {
                return false;
            }
            let own = self.reserve[dimension].saturating_sub(held[dimension]);
            if amount[dimension].saturating_sub(own) > shared[dimension] {
                return false;
            }
        }
        true
    }

    pub fn try_acquire_all(self, amount: [usize; N]) -> bool {
        let held = self.held.get();
        let shared = self.shared.get();
        let mut next = [0; N];
        let mut borrowed = [0; N];
        for dimension in 0..N {
            let Some(next_held) = held[dimension].checked_add(amount[dimension]) else {
                return false;
            };
            next[dimension] = next_held;
            let own = self.reserve[dimension].saturating_sub(held[dimension]);
            borrowed[dimension] = amount[dimension].saturating_sub(own);
            if borrowed[dimension] > shared[dimension] {
                return false;
            }
        }
        self.held.set(next);
        self.shared.set(array::from_fn(|dimension| {
            shared[dimension] - borrowed[dimension]
        }));
        true
    }

    pub fn release_all(self, amount: [usize; N]) {
        let held = self.held.get();
        for dimension in 0..N {
            assert!(
                held[dimension] >= amount[dimension],
                "cannot release credits that are not held"
            );
        }
        let returned: [usize; N] = array::from_fn(|dimension| {
            amount[dimension].min(held[dimension].saturating_sub(self.reserve[dimension]))
        });
        self.held.set(array::from_fn(|dimension| {
            held[dimension] - amount[dimension]
        }));
        let shared = self.shared.get();
        self.shared.set(array::from_fn(|dimension| {
            shared[dimension] + returned[dimension]
        }));
    }
}

impl<const N: usize> Credits<N> {
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

    fn with_reserve_per_lane(capacity: [usize; N], lane_count: usize, reserve: [usize; N]) -> Self {
        assert!(N > 0, "credit dimension count must be positive");
        assert!(lane_count > 0, "credit lane count must be positive");
        let reserved: [usize; N] = array::from_fn(|dimension| {
            assert!(
                reserve[dimension] <= capacity[dimension] / lane_count,
                "credit reserve exceeds capacity"
            );
            reserve[dimension] * lane_count
        });
        Self {
            used: cell::Cell::new([0; N]),
            pool: Pool::new(array::from_fn(|dimension| {
                capacity[dimension] - reserved[dimension]
            })),
            held: (0..lane_count).map(|_| cell::Cell::new([0; N])).collect(),
            reserve,
        }
    }

    fn credit(&self, lane: usize) -> Option<Lane<'_, N>> {
        Some(Lane {
            shared: &self.pool.shared,
            held: self.held.get(lane)?,
            reserve: self.reserve,
        })
    }

    fn can_acquire_all(&self, lane: usize, amount: [usize; N]) -> bool {
        self.credit(lane)
            .is_some_and(|credit| credit.can_acquire_all(amount))
    }

    /// Atomically checks and acquires every resource dimension.
    pub fn try_acquire_all(&self, lane: usize, amount: [usize; N]) -> bool {
        let Some(credit) = self.credit(lane) else {
            return false;
        };
        if !credit.try_acquire_all(amount) {
            return false;
        }
        let used = self.used.get();
        self.used.set(array::from_fn(|dimension| {
            used[dimension] + amount[dimension]
        }));
        true
    }

    /// Releases every resource dimension as one state transition.
    pub fn release_all(&self, lane: usize, amount: [usize; N]) {
        let credit = Lane {
            shared: &self.pool.shared,
            held: &self.held[lane],
            reserve: self.reserve,
        };
        credit.release_all(amount);
        let used = self.used.get();
        self.used.set(array::from_fn(|dimension| {
            used[dimension] - amount[dimension]
        }));
    }

    fn held_all_by(&self, lane: usize) -> Option<[usize; N]> {
        self.held.get(lane).map(cell::Cell::get)
    }

    fn reserved_all_for(&self, lane: usize) -> Option<[usize; N]> {
        self.held.get(lane).map(|_| self.reserve)
    }
}

impl Credits {
    pub fn with_reserve(capacity: usize, lane_count: usize, reserve_per_lane: usize) -> Self {
        Self::with_reserve_per_lane([capacity], lane_count, [reserve_per_lane])
    }

    pub fn used(&self) -> usize {
        self.used.get()[0]
    }

    pub fn held_by(&self, lane: usize) -> Option<usize> {
        self.held_all_by(lane).map(|held| held[0])
    }

    pub fn reserved_for(&self, lane: usize) -> Option<usize> {
        self.reserved_all_for(lane).map(|reserved| reserved[0])
    }

    pub fn shared_available(&self) -> usize {
        self.pool.shared()[0]
    }

    pub fn can_acquire(&self, lane: usize, amount: usize) -> bool {
        self.can_acquire_all(lane, [amount])
    }

    pub fn try_acquire(&self, lane: usize, amount: usize) -> bool {
        self.try_acquire_all(lane, [amount])
    }

    pub fn release(&self, lane: usize, amount: usize) {
        self.release_all(lane, [amount]);
    }
}
