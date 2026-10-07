use std::marker::PhantomData;

use crate::{
    base::marker::ThreadBound,
    depot::map::{
        self, resident,
        resident::{Key, Table},
    },
};

type Brand<'id> = PhantomData<(fn(&'id mut ()) -> &'id mut (), ThreadBound)>;

/// Retains the owner while published records cannot be retired.
pub struct Scope<'a, 'id, T, H, const MAX: u32> {
    table: &'a mut Table<T, H, MAX>,
    brand: Brand<'id>,
}

/// One published record under this exact projecting scope.
pub struct Lease<'id, const MAX: u32> {
    key: Key<MAX>,
    brand: Brand<'id>,
}

/// Sole rollback authority for a record newly published by this scope.
pub struct Fresh<'id, const MAX: u32> {
    key: Key<MAX>,
    brand: Brand<'id>,
}

/// Consuming the projecting scope ends every ordinary lease's access.
pub struct Retirement<'a, 'id, T, H, const MAX: u32> {
    table: &'a mut Table<T, H, MAX>,
    brand: Brand<'id>,
}

/// Keeps the actual owner loan until a record is published or released.
pub struct Occupied<'a, 'id, T, H, const MAX: u32> {
    entry: resident::Occupied<'a, T, H, MAX>,
    brand: Brand<'id>,
    fresh: bool,
}

pub struct Vacant<'a, 'id, T, H, const MAX: u32> {
    entry: resident::Vacant<'a, T, H, MAX>,
    brand: Brand<'id>,
}

impl<'a, 'id, T, H: map::Hasher<resident::Id>, const MAX: u32> Scope<'a, 'id, T, H, MAX> {
    pub(super) fn new(table: &'a mut Table<T, H, MAX>) -> Self {
        Self {
            brand: PhantomData,
            table,
        }
    }

    pub fn entry(
        &mut self,
        id: resident::Id,
    ) -> map::Entry<Occupied<'_, 'id, T, H, MAX>, Option<Vacant<'_, 'id, T, H, MAX>>> {
        match self.table.entry(id) {
            map::Entry::Occupied(entry) => map::Entry::Occupied(Occupied {
                entry,
                brand: self.brand,
                fresh: false,
            }),
            map::Entry::Vacant(entry) => map::Entry::Vacant(entry.map(|entry| Vacant {
                entry,
                brand: self.brand,
            })),
        }
    }

    pub fn exact(
        &mut self,
        key: Key<MAX>,
        id: resident::Id,
    ) -> Result<Occupied<'_, 'id, T, H, MAX>, &Table<T, H, MAX>> {
        match self.table.exact(key, id) {
            Ok(entry) => Ok(Occupied {
                entry,
                brand: self.brand,
                fresh: false,
            }),
            Err(table) => Err(table),
        }
    }

    pub fn get_mut(&mut self, lease: &Lease<'id, MAX>) -> &mut T {
        // SAFETY: the same retained scope keeps this record live, and the
        // projection borrows the scope exclusively for its entire lifetime.
        unsafe {
            self.table
                .records
                .get_unchecked_mut(lease.key.index() as usize)
                .value
                .assume_init_mut()
        }
    }

    pub fn retire(self) -> Retirement<'a, 'id, T, H, MAX> {
        Retirement {
            table: self.table,
            brand: self.brand,
        }
    }
}

impl<'a, 'id, T, H: map::Hasher<resident::Id>, const MAX: u32> Vacant<'a, 'id, T, H, MAX> {
    pub fn insert(self, value: T) -> Occupied<'a, 'id, T, H, MAX> {
        Occupied {
            entry: self.entry.insert(value),
            brand: self.brand,
            fresh: true,
        }
    }
}

impl<T, H: map::Hasher<resident::Id>, const MAX: u32> Occupied<'_, '_, T, H, MAX> {
    pub fn id(&self) -> resident::Id {
        self.entry.id()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.entry.get_mut()
    }

    /// Unpublished new records have no outstanding lease; older records stay live.
    pub fn release(self) -> Option<T> {
        self.fresh.then(|| self.entry.remove())
    }
}

impl<'id, T, H: map::Hasher<resident::Id>, const MAX: u32> Occupied<'_, 'id, T, H, MAX> {
    pub fn publish(self) -> (Lease<'id, MAX>, Option<Fresh<'id, MAX>>) {
        let key = self.entry.key();
        let lease = Lease {
            key,
            brand: self.brand,
        };
        let fresh = if self.fresh {
            Some(Fresh {
                key,
                brand: self.brand,
            })
        } else {
            None
        };
        (lease, fresh)
    }
}

impl<const MAX: u32> Lease<'_, MAX> {
    pub const fn key(&self) -> Key<MAX> {
        self.key
    }
}

impl<'id, T, H: map::Hasher<resident::Id>, const MAX: u32> Retirement<'_, 'id, T, H, MAX> {
    /// Published slots stay live in Scope, and affine Fresh tokens name distinct insertions.
    /// Consuming one token retains its original owner after prior retirements of other records.
    pub fn occupied(&mut self, fresh: Fresh<'id, MAX>) -> resident::Occupied<'_, T, H, MAX> {
        resident::Occupied {
            table: self.table,
            bucket: None,
            index: fresh.key.slot(),
        }
    }
}

const _: () = {
    assert!(std::mem::size_of::<Lease<'static, { u32::MAX }>>() == 8);
    assert!(std::mem::size_of::<Option<Fresh<'static, { u32::MAX }>>>() == 8);
};
