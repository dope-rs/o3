use crate::marker::ThreadBound;

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Links {
    next: u32,
    prev: u32,
}

const VACANT: Links = Links {
    next: NONE,
    prev: NONE,
};

pub struct RoundRobinSet {
    links: Box<[Links]>,
    head: u32,
    len: usize,
    _thread: ThreadBound,
}

impl RoundRobinSet {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            u32::try_from(capacity).is_ok(),
            "round-robin set capacity overflow"
        );
        Self {
            links: vec![VACANT; capacity].into_boxed_slice(),
            head: NONE,
            len: 0,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn capacity(&self) -> usize {
        self.links.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn contains(&self, index: usize) -> bool {
        self.links
            .get(index)
            .is_some_and(|links| links.next != NONE)
    }

    pub fn insert(&mut self, index: usize) -> bool {
        let Some(links) = self.links.get(index) else {
            return false;
        };
        if links.next != NONE {
            return false;
        }
        self.len += 1;
        if self.head == NONE {
            self.head = index as u32;
            self.links[index] = Links {
                next: index as u32,
                prev: index as u32,
            };
            return true;
        }
        let first = self.head as usize;
        let last = self.links[first].prev as usize;
        self.links[index] = Links {
            next: first as u32,
            prev: last as u32,
        };
        self.links[last].next = index as u32;
        self.links[first].prev = index as u32;
        true
    }

    pub fn remove(&mut self, index: usize) -> bool {
        let Some(links) = self.links.get(index) else {
            return false;
        };
        if links.next == NONE {
            return false;
        }
        self.len -= 1;
        let following = links.next;
        let preceding = links.prev;
        if following == index as u32 {
            self.head = NONE;
        } else {
            self.links[preceding as usize].next = following;
            self.links[following as usize].prev = preceding;
            if self.head == index as u32 {
                self.head = following;
            }
        }
        self.links[index] = VACANT;
        true
    }

    pub fn next_index(&mut self) -> Option<usize> {
        if self.head == NONE {
            return None;
        }
        let index = self.head as usize;
        self.head = self.links[index].next;
        Some(index)
    }
}
