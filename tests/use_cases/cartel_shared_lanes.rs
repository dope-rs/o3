#![forbid(unsafe_code)]

use o3::cell::{RegionCell, RegionToken};
use o3::collections::{LinkedArena, Slab, SlabKey};
use o3::mem::FairCredits;

struct State<T> {
    items: LinkedArena<(T, usize)>,
    credits: FairCredits,
    weights: Box<[usize]>,
}

impl<T> State<T> {
    fn with_capacity(capacity: usize, lanes: usize) -> Self {
        assert!(lanes > 0);
        assert!(capacity >= lanes);
        Self {
            items: LinkedArena::with_capacity(capacity, lanes),
            credits: FairCredits::with_reserve(capacity, lanes, 1),
            weights: vec![0; lanes].into_boxed_slice(),
        }
    }

    fn can_push(&self, lane: usize) -> bool {
        !self.items.is_full() && self.credits.can_acquire(lane, 1)
    }

    fn try_push(&mut self, lane: usize, item: T, weight: usize) -> Result<(), T> {
        if self.weights.get(lane).is_none() || !self.credits.try_acquire(lane, 1) {
            return Err(item);
        }
        if let Err((item, _)) = self.items.push_back(lane, (item, weight)) {
            self.credits.release(lane, 1);
            return Err(item);
        }
        self.weights[lane] = self.weights[lane].saturating_add(weight);
        Ok(())
    }

    fn pop_front(&mut self, lane: usize) -> Option<(T, usize)> {
        let (item, weight) = self.items.pop_front(lane)?;
        self.credits.release(lane, 1);
        self.weights[lane] = self.weights[lane].saturating_sub(weight);
        Some((item, weight))
    }

    fn restore_front(&mut self, lane: usize, item: T, weight: usize) {
        assert!(self.credits.try_acquire(lane, 1));
        assert!(self.items.push_front(lane, (item, weight)).is_ok());
        self.weights[lane] = self.weights[lane].saturating_add(weight);
    }
}

struct ReplyCredits {
    resources: FairCredits<2>,
}

enum ReplyEntryTag {}

struct ReplyEntry {
    live: bool,
    ordered: bool,
}

struct ReplyStore<T> {
    entries: Slab<ReplyEntry, ReplyEntryTag>,
    items: LinkedArena<T>,
    order: LinkedArena<SlabKey<ReplyEntryTag>>,
}

impl<T> ReplyStore<T> {
    fn with_capacity(capacity: usize, lanes: usize) -> Self {
        Self {
            entries: Slab::with_capacity(capacity),
            items: LinkedArena::with_capacity(capacity, capacity),
            order: LinkedArena::with_capacity(capacity, lanes),
        }
    }

    fn register(&mut self, lane: usize) -> Option<SlabKey<ReplyEntryTag>> {
        let key = self
            .entries
            .insert(ReplyEntry {
                live: true,
                ordered: true,
            })
            .ok()?;
        assert!(self.order.push_back(lane, key).is_ok());
        Some(key)
    }

    fn try_push(&mut self, key: SlabKey<ReplyEntryTag>, item: T) -> Result<(), T> {
        if self.entries.get(key).is_none() {
            return Err(item);
        }
        self.items.push_back(key.index() as usize, item)
    }

    fn retire(&mut self, key: SlabKey<ReplyEntryTag>) {
        let Some(entry) = self.entries.get_mut(key) else {
            return;
        };
        if !entry.live {
            return;
        }
        entry.live = false;
        let ordered = entry.ordered;
        while self.items.pop_front(key.index() as usize).is_some() {}
        if !ordered {
            assert!(self.entries.remove(key).is_some());
        }
    }

    fn complete(&mut self, lane: usize) {
        let Some(key) = self.order.pop_front(lane) else {
            return;
        };
        let Some(entry) = self.entries.get_mut(key) else {
            return;
        };
        entry.ordered = false;
        if !entry.live {
            assert!(self.entries.remove(key).is_some());
        }
    }
}

impl ReplyCredits {
    fn new(rows: usize, bytes: usize, lanes: usize) -> Self {
        Self {
            resources: FairCredits::from_capacities([rows, bytes], lanes),
        }
    }

    fn try_push(&mut self, lane: usize, rows: usize, bytes: usize) -> bool {
        self.resources.try_acquire_all(lane, [rows, bytes])
    }

    fn pop(&mut self, lane: usize, rows: usize, bytes: usize) {
        self.resources.release_all(lane, [rows, bytes]);
    }
}

struct QueueArena<'region, T> {
    state: RegionCell<'region, State<T>>,
    lanes: usize,
}

impl<'region, T: Unpin> QueueArena<'region, T> {
    fn with_capacity(capacity: usize, lanes: usize) -> Self {
        Self {
            state: RegionCell::new(State::<T>::with_capacity(capacity, lanes)),
            lanes,
        }
    }

    fn lane(&self, lane: usize) -> QueueLane<'_, 'region, T> {
        assert!(lane < self.lanes);
        QueueLane { arena: self, lane }
    }
}

struct QueueLane<'arena, 'region, T> {
    arena: &'arena QueueArena<'region, T>,
    lane: usize,
}

impl<T> Copy for QueueLane<'_, '_, T> {}

impl<T> Clone for QueueLane<'_, '_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'region, T: Unpin> QueueLane<'_, 'region, T> {
    fn can_push(self, token: &RegionToken<'region>) -> bool {
        self.arena.state.borrow(token).can_push(self.lane)
    }

    fn len(self, token: &RegionToken<'region>) -> usize {
        self.arena.state.borrow(token).items.lane_len(self.lane)
    }

    fn weight(self, token: &RegionToken<'region>) -> usize {
        self.arena.state.borrow(token).weights[self.lane]
    }

    fn try_push(self, token: &mut RegionToken<'region>, item: T, weight: usize) -> Result<(), T> {
        self.arena
            .state
            .borrow_mut(token)
            .try_push(self.lane, item, weight)
    }

    fn pop_front(self, token: &mut RegionToken<'region>) -> Option<T> {
        self.arena
            .state
            .borrow_mut(token)
            .pop_front(self.lane)
            .map(|(item, _)| item)
    }

    fn drain(self, token: &mut RegionToken<'region>, mut consume: impl FnMut(T) -> Result<(), T>) {
        while let Some((item, weight)) = self.arena.state.borrow_mut(token).pop_front(self.lane) {
            if let Err(item) = consume(item) {
                self.arena
                    .state
                    .borrow_mut(token)
                    .restore_front(self.lane, item, weight);
                break;
            }
        }
    }
}

#[test]
fn cartel_request_lanes_share_fixed_storage_without_losing_reserve_or_fifo_order() {
    RegionToken::scope(|mut token| {
        let arena = QueueArena::with_capacity(4, 2);
        let left = arena.lane(0);
        let right = arena.lane(1);

        assert_eq!(left.try_push(&mut token, 10, 2), Ok(()));
        assert_eq!(left.try_push(&mut token, 11, 3), Ok(()));
        assert_eq!(left.try_push(&mut token, 12, 5), Ok(()));
        assert!(!left.can_push(&token));
        assert!(right.can_push(&token));
        assert_eq!(left.try_push(&mut token, 13, 7), Err(13));
        assert_eq!(right.try_push(&mut token, 20, 11), Ok(()));
        assert_eq!(left.len(&token), 3);
        assert_eq!(right.len(&token), 1);
        assert_eq!(left.weight(&token), 10);
        assert_eq!(right.weight(&token), 11);

        left.drain(
            &mut token,
            |item| if item == 11 { Err(item) } else { Ok(()) },
        );
        assert_eq!(left.len(&token), 2);
        assert_eq!(left.weight(&token), 8);
        assert_eq!(left.pop_front(&mut token), Some(11));
        assert_eq!(left.pop_front(&mut token), Some(12));
        assert_eq!(right.pop_front(&mut token), Some(20));
        assert_eq!(left.pop_front(&mut token), None);
    });
}

#[test]
fn cartel_reply_rows_and_bytes_are_reserved_atomically() {
    let mut credits = ReplyCredits::new(8, 80, 2);

    assert!(credits.try_push(0, 6, 60));
    assert!(!credits.try_push(1, 2, 21));
    assert!(credits.try_push(1, 2, 20));
    credits.pop(0, 6, 60);
    credits.pop(1, 2, 20);
    assert!(credits.try_push(0, 6, 60));
    assert!(credits.try_push(1, 2, 20));
}

#[test]
fn cartel_reply_entry_generation_rejects_a_retired_handle_after_index_reuse() {
    let mut replies = ReplyStore::with_capacity(1, 1);
    let retired = replies.register(0).unwrap();
    assert_eq!(replies.try_push(retired, 7), Ok(()));
    replies.retire(retired);
    replies.complete(0);

    let current = replies.register(0).unwrap();
    assert_eq!(current.index(), retired.index());
    assert_ne!(current.generation(), retired.generation());
    assert_eq!(replies.try_push(retired, 8), Err(8));
    assert_eq!(replies.try_push(current, 9), Ok(()));
}
