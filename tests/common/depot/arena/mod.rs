use o3::depot::{
    self,
    arena::{
        hash::runtime::{self, entry},
        slab::key,
    },
};

pub mod linked;
pub mod pool;
pub mod slab;

#[derive(Default)]
pub struct Inline {
    pub len: usize,
    pub bytes: [u8; 32],
}

pub enum Tag {}

pub struct Borrowed<'a>(pub &'a u8);

#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: u32,
    pub observed: u64,
}

pub struct Item {
    pub id: u8,
    pub revision: u64,
}

impl Item {
    pub fn hash(id: u8) -> u64 {
        u64::from(id % 4) | 15
    }
}

pub struct Slot<'a> {
    map: &'a mut runtime::Map<u64>,
    key: key::Handle<runtime::Domain>,
}

unsafe impl depot::raw::Source<runtime::Indexed> for Slot<'_> {
    type Key = key::Handle<runtime::Domain>;

    fn key(&self) -> Self::Key {
        self.key
    }
}

pub struct Member<'a>(pub entry::Live<'a, u64, (), entry::Occupied>);

unsafe impl depot::raw::Source<runtime::Indexed> for Member<'_> {
    type Key = key::Handle<runtime::Domain>;

    fn key(&self) -> Self::Key {
        self.0.key()
    }
}

impl<'a> Slot<'a> {
    /// # Safety
    /// `key` must name a live initialized slot in the retained exact owner.
    pub unsafe fn new(map: &'a mut runtime::Map<u64>, key: key::Handle<runtime::Domain>) -> Self {
        Self { map, key }
    }

    pub fn into_owner(self) -> &'a mut runtime::Map<u64> {
        self.map
    }
}
