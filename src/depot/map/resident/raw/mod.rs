pub(super) use record::Record;

use crate::{
    alloc::raw::VecExt as _,
    depot::{
        map::{self, resident},
        queue::links,
    },
};

mod record;

const FREE: u64 = 1 << 63;

/// # Safety
/// This owner retains both the vacant bucket and an available record credit.
pub(super) unsafe fn insert<T, H, const MAX: u32>(
    table: &mut resident::Table<T, H, MAX>,
    bucket: map::raw::Vacant,
    id: resident::Id,
    value: T,
) -> links::Index {
    let index = match table.free {
        Some(index) => {
            let record = unsafe { table.records.get_unchecked_mut(index.get()) };
            table.free = links::Index::decode(record.id as u32);
            record.id = id.get();
            record.value.write(value);
            index
        }
        None => {
            let next = table.records.len();
            // SAFETY: available credit with no recycled slot proves this
            // initialized prefix is shorter than the original allocation.
            let index = unsafe { links::Index::new_usize(next) };
            // SAFETY: the same retained credit admits this exact vector's next slot.
            unsafe { table.records.vacancy() }.insert(Record::new(id, value));
            index
        }
    };
    table.len = table.len.wrapping_add(1);
    unsafe { *table.buckets.get_unchecked_mut(bucket.index()) = index.raw() };
    index
}

/// # Safety
/// The retained bucket names this live record in the same exclusive owner.
pub(super) unsafe fn remove<T, H: map::Hasher<resident::Id>, const MAX: u32>(
    table: &mut resident::Table<T, H, MAX>,
    bucket: map::raw::Occupied,
    index: links::Index,
) -> T {
    let offset = bucket.index();
    unsafe { *table.buckets.get_unchecked_mut(offset) = links::Index::END };
    unsafe { map::raw::close(table, map::raw::Vacant::new(offset)) };
    let record = unsafe { table.records.get_unchecked_mut(index.get()) };
    let value = unsafe { record.value.assume_init_read() };
    if let Some(next) = record.generation.next() {
        record.generation = next;
        record.id = FREE | u64::from(links::Index::encode(table.free));
        table.free = Some(index);
    } else {
        record.id = FREE | u64::from(links::Index::END);
        table.usable = table.usable.wrapping_sub(1);
    }
    table.len = table.len.wrapping_sub(1);
    value
}
