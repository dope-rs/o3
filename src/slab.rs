use crate::id::{SlotGen, SlotId};

const NO_FREE: u32 = u32::MAX;

#[inline]
fn bump(gn: SlotGen) -> SlotGen {
    gn.checked_add(1).unwrap_or(SlotGen::MIN)
}

enum Entry<T> {
    Free { next: u32, gn: SlotGen },
    Occupied { value: T, gn: SlotGen },
}

impl<T> Entry<T> {
    fn gn(&self) -> SlotGen {
        match self {
            Self::Free { gn, .. } | Self::Occupied { gn, .. } => *gn,
        }
    }
}

pub struct Slab<T> {
    slots: Vec<Entry<T>>,
    next_free: u32,
    len: usize,
    cap: usize,
}

impl<T> Slab<T> {
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            slots: Vec::new(),
            next_free: NO_FREE,
            len: 0,
            cap,
        }
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push_free(&mut self, next: u32) {
        self.slots.push(Entry::Free {
            next,
            gn: SlotGen::MIN,
        });
    }

    fn grow_through(&mut self, index: usize) {
        while self.slots.len() <= index {
            let here = self.slots.len() as u32;
            let next = if here as usize == index {
                NO_FREE
            } else {
                self.next_free
            };
            self.push_free(next);
            if here as usize != index {
                self.next_free = here;
            }
        }
    }

    fn pop_free(&mut self) -> Option<(usize, SlotGen)> {
        if self.next_free == NO_FREE {
            if self.slots.len() >= self.cap {
                return None;
            }
            let idx = self.slots.len();
            self.push_free(NO_FREE);
            return Some((idx, SlotGen::MIN));
        }
        let idx = self.next_free as usize;
        let Entry::Free { next, gn } = self.slots[idx] else {
            panic!("slab invariant violated: freelist points to occupied slot");
        };
        self.next_free = next;
        Some((idx, gn))
    }

    pub fn alloc(&mut self, value: T) -> Option<SlotId> {
        let (idx, gn) = self.pop_free()?;
        self.slots[idx] = Entry::Occupied { value, gn };
        self.len += 1;
        Some(SlotId::from_parts(idx as u32, gn))
    }

    // Fills a caller-chosen index, bypassing the freelist; never mix with
    // alloc/reserve on the same slab.
    pub fn place_at<F: FnOnce(SlotId) -> T>(&mut self, slot: u32, make: F) -> SlotId {
        let i = slot as usize;
        assert!(i < self.cap, "slab::place_at index out of range");
        self.grow_through(i);
        let gn = bump(self.slots[i].gn());
        if matches!(self.slots[i], Entry::Free { .. }) {
            self.len += 1;
        }
        let id = SlotId::from_parts(slot, gn);
        self.slots[i] = Entry::Occupied {
            value: make(id),
            gn,
        };
        id
    }

    pub fn reserve(&mut self) -> Option<Reservation<'_, T>> {
        let (idx, gn) = self.pop_free()?;
        let gn = bump(gn);
        Some(Reservation {
            slab: self,
            index: idx as u32,
            gn,
        })
    }

    pub fn get(&self, id: SlotId) -> Option<&T> {
        match self.slots.get(id.slot() as usize)? {
            Entry::Occupied { value, gn } if *gn == id.generation() => Some(value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, id: SlotId) -> Option<&mut T> {
        match self.slots.get_mut(id.slot() as usize)? {
            Entry::Occupied { value, gn } if *gn == id.generation() => Some(value),
            _ => None,
        }
    }

    pub fn remove(&mut self, id: SlotId) -> bool {
        let i = id.slot() as usize;
        let Some(Entry::Occupied { gn, .. }) = self.slots.get(i) else {
            return false;
        };
        if *gn != id.generation() {
            return false;
        }
        let next_gn = bump(*gn);
        self.slots[i] = Entry::Free {
            next: self.next_free,
            gn: next_gn,
        };
        self.next_free = id.slot();
        self.len -= 1;
        true
    }

    pub fn at_index(&self, slot: u32) -> Option<(&T, SlotGen)> {
        match self.slots.get(slot as usize)? {
            Entry::Occupied { value, gn } => Some((value, *gn)),
            Entry::Free { .. } => None,
        }
    }

    pub fn at_index_mut(&mut self, slot: u32) -> Option<&mut T> {
        match self.slots.get_mut(slot as usize)? {
            Entry::Occupied { value, .. } => Some(value),
            Entry::Free { .. } => None,
        }
    }

    pub fn generation(&self, slot: u32) -> Option<SlotGen> {
        Some(self.slots.get(slot as usize)?.gn())
    }
}

pub struct Reservation<'a, T> {
    slab: &'a mut Slab<T>,
    index: u32,
    gn: SlotGen,
}

impl<T> Reservation<'_, T> {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn generation(&self) -> SlotGen {
        self.gn
    }

    pub fn fill(self, value: T) -> SlotId {
        let i = self.index as usize;
        let gn = self.gn;
        self.slab.slots[i] = Entry::Occupied { value, gn };
        self.slab.len += 1;
        let id = SlotId::from_parts(self.index, gn);
        std::mem::forget(self);
        id
    }
}

impl<T> Drop for Reservation<'_, T> {
    fn drop(&mut self) {
        self.slab.next_free = self.index;
    }
}
