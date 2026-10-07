use o3::depot::{self, array::bits};

pub mod arena;
pub mod compact;
pub mod heap;
pub mod index;
pub mod map;
pub mod model;
pub mod queue;
pub mod set;

#[repr(C)]
pub struct CompactBytes {
    pub entries: [u8; 65],
    pub len: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Fault {
    Element(usize),
}

#[derive(Clone, Copy)]
pub struct Index(usize);

impl Index {
    pub fn from_mask(mask: u16) -> bits::Set<Self, 4> {
        let mut set = bits::Set::EMPTY;
        for index in 0..4 {
            if mask & (1 << index) != 0 {
                set.insert(Self(index));
            }
        }
        set
    }
}

unsafe impl depot::raw::BitIndex<4> for Index {
    fn index(self) -> usize {
        self.0
    }

    unsafe fn from_index(index: usize) -> Self {
        Self(index)
    }
}
