use std::cell::Cell;

use o3::{
    alloc,
    depot::map::{self, resident},
};

pub struct Directory {
    pub table: map::raw::Table<u32>,
    shape: map::raw::Shape<u32>,
}

impl Directory {
    pub fn try_new(plan: map::Plan) -> Result<Self, alloc::Error> {
        let layout = map::raw::Layout::try_plan(plan)?;
        let shape = layout.shape();
        let buckets = layout.buckets();
        // SAFETY: this admitted layout retains the exact field sequence.
        let bundle = unsafe { alloc::raw::Recipe::build(layout) };
        // SAFETY: this owner retains all planes from this exact allocation.
        let table = unsafe { map::raw::Table::from_fields(bundle.try_allocate()?, buckets) };
        Ok(Self { table, shape })
    }
}

// SAFETY: the original exclusive loan retains this directory and its initialized payloads.
unsafe impl map::raw::Query for &mut Directory {
    type Payload = u32;
    type Value = u32;
    type Occupied = map::Found<Self>;

    fn table(&self) -> &map::raw::Table<u32> {
        &self.table
    }

    fn value(entry: map::Found<&Self>) -> &u32 {
        entry.project().1
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        entry
    }
}

// SAFETY: the vacancy and its exact hash stay paired with the original exclusive owner.
unsafe impl map::raw::Insertion for &mut Directory {
    type Vacant = (map::Vacancy<Self>, u64);

    fn vacant(entry: map::Vacancy<Self>, hash: u64) -> Option<Self::Vacant> {
        Some((entry, hash))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        // SAFETY: u32 payloads need no destruction; the retained shape recreates
        // the exact allocation layout after the owner's final loan has ended.
        unsafe { alloc::raw::Recipe::build(self.shape.layout()).deallocate(self.table.base()) };
    }
}

pub enum Sent {}

#[derive(Clone, Copy)]
pub struct Seed(pub u64);

impl map::Hasher<u64> for Seed {
    fn hash(&self, key: &u64) -> u64 {
        mix(*key ^ self.0)
    }
}

impl map::Hasher<u32> for Seed {
    fn hash(&self, key: &u32) -> u64 {
        mix(u64::from(*key) ^ self.0)
    }
}

#[derive(Clone, Copy)]
pub struct Collision;

impl map::Hasher<u64> for Collision {
    fn hash(&self, _: &u64) -> u64 {
        0
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
#[repr(align(64))]
pub struct Aligned(pub u64);

pub struct AlignedSeed;

impl map::Hasher<Aligned> for AlignedSeed {
    fn hash(&self, key: &Aligned) -> u64 {
        key.0
    }
}

pub struct Model {
    pub key: u64,
    pub value: u64,
    pub priority: (u64, u64),
}

fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

pub struct ResidentHash<'a>(pub &'a Cell<u64>);

impl map::Hasher<resident::Id> for ResidentHash<'_> {
    fn hash(&self, _: &resident::Id) -> u64 {
        self.0.get()
    }
}
