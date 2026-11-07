use std::{
    alloc::{Layout, LayoutError},
    array::from_fn,
    marker, mem,
};

use o3::alloc::{self, array};

pub mod owner;
pub mod system;
pub mod tail;

#[repr(align(256))]
#[derive(Clone, Copy)]
pub struct AlignedZst;

#[repr(align(64))]
pub struct AlignedUnit;

#[repr(align(64))]
pub struct Aligned(pub u8);

#[repr(align(64))]
pub struct Header(pub u32);

pub struct Plan<R, T, E = ()> {
    roots: usize,
    tails: usize,
    offset: usize,
    layout: Layout,
    value: marker::PhantomData<(R, T, E)>,
}

impl<R, T, E> Plan<R, T, E> {
    pub fn try_new(roots: usize, tails: usize) -> Result<Self, LayoutError> {
        let root = Layout::array::<R>(roots)?;
        let tail = Layout::array::<T>(tails)?;
        let (layout, offset) = root.extend(tail)?;
        let (layout, _) = layout.extend(Layout::array::<E>(roots)?)?;
        Ok(Self {
            roots,
            tails,
            offset,
            layout: layout.pad_to_align(),
            value: marker::PhantomData,
        })
    }
}

// SAFETY: construction checks each exact plane layout and their extensions;
// the final plane contains only zero-sized, byte-aligned units.
unsafe impl<R, T, E> alloc::raw::Shape for Plan<R, T, E> {
    type Root = R;
    type Tail = T;
    type Extra = E;
    type Last = ();

    fn len(&self) -> usize {
        self.roots
    }

    fn tail_len(&self) -> usize {
        self.tails
    }

    fn offset(&self) -> usize {
        self.offset
    }

    fn layout(&self) -> Layout {
        self.layout
    }
}

pub struct Rows;

// SAFETY: three rows of these four fixed, small types have a representable
// layout; Shape's default offsets align and separate every plane.
unsafe impl alloc::raw::Shape for Rows {
    type Root = u32;
    type Tail = mem::MaybeUninit<u64>;
    type Extra = u8;
    type Last = u16;

    fn len(&self) -> usize {
        3
    }
}

// SAFETY: Shape's default tail length equals the other three plane lengths.
unsafe impl alloc::raw::Rows for Rows {}

/// Empty, representable owner used to inspect generic row bounds without allocating.
pub struct EmptyRows<R, T, E, L>(marker::PhantomData<(R, T, E, L)>);

// SAFETY: each plane is empty, and Shape's defaults retain its required alignment.
unsafe impl<R, T, E, L> alloc::raw::Shape for EmptyRows<R, T, E, L> {
    type Root = R;
    type Tail = T;
    type Extra = E;
    type Last = L;

    fn len(&self) -> usize {
        0
    }
}

// SAFETY: all four planes have the same zero extent.
unsafe impl<R, T, E, L> alloc::raw::Rows for EmptyRows<R, T, E, L> {}

type Slots<S = ()> = array::Block<mem::MaybeUninit<Box<u64>>, usize, S>;

pub struct Wrapped {
    pub block: Slots<(usize, usize)>,
}

impl Wrapped {
    pub fn try_new() -> Result<Self, alloc::Error> {
        let mut owner = Self {
            block: array::Block::try_new(5, (3, 0))?,
        };
        for (index, value) in [(3, 10), (4, 20), (0, 30), (1, 40)] {
            owner.block[index].write(alloc::BoxExt::try_new(value)?);
            owner.block.state_mut().1 += 1;
        }
        Ok(owner)
    }

    pub fn values(&self) -> [u64; 4] {
        let (head, _) = *self.block.state();
        from_fn(|index| {
            let slot = &self.block[(head + index) % self.block.len()];
            // SAFETY: the owner retains this initialized logical prefix.
            **unsafe { slot.assume_init_ref() }
        })
    }
}

impl Drop for Wrapped {
    fn drop(&mut self) {
        let (head, len) = *self.block.state();
        let cap = self.block.len();
        for index in 0..len {
            // SAFETY: terminal ownership retires each initialized entry once.
            unsafe { self.block[(head + index) % cap].assume_init_drop() };
        }
    }
}
