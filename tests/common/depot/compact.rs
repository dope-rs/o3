use std::{cell::Cell, mem};

use o3::{
    alloc::{self, BoxSliceExt as _, VecExt as _, tail},
    depot,
    depot::map::compact,
};

pub struct Store {
    layout: compact::Layout,
    bytes: tail::Block<(), u8>,
    records: Box<[(u64, bool)]>,
    pub hash_reads: Cell<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Layout(compact::Error),
    Overflow(alloc::Overflow),
    Allocation(alloc::Exhausted),
    Records(alloc::Error),
    Domain { index: usize, capacity: usize },
    Occupied { index: usize },
    Missing { index: usize },
}

impl Store {
    pub fn try_new(capacity: usize) -> Result<Self, Error> {
        let (layout, _) = compact::Layout::try_new(capacity, 7).map_err(Error::Layout)?;
        let plan = compact::raw::Plan::new(layout).map_err(Error::Overflow)?;
        Ok(Self {
            layout,
            bytes: plan.try_allocate(()).map_err(Error::Allocation)?,
            records: Box::try_repeat(capacity, (0, false)).map_err(Error::Records)?,
            hash_reads: Cell::new(0),
        })
    }

    pub fn try_insert(&mut self, index: usize, hash: u64) -> Result<Insert<'_>, Error> {
        let capacity = self.records.len();
        let record = self
            .records
            .get_mut(index)
            .ok_or(Error::Domain { index, capacity })?;
        if record.1 {
            return Err(Error::Occupied { index });
        }
        *record = (hash, true);
        Ok(Insert {
            store: self,
            index: index as u32,
        })
    }

    /// Each private record can appear once, so its full capacity bounds the scan without growth.
    pub fn try_find(&self, hash: u64) -> Result<Vec<usize>, alloc::Error> {
        use compact::Access as _;
        let mut found = Vec::try_new(self.records.len())?;
        Read(self).scan(hash, |value| found.push(value));
        found.sort_unstable();
        Ok(found)
    }

    pub fn try_remove(&mut self, hash: u64, index: usize) -> Result<Option<u64>, Error> {
        use compact::Access as _;
        Remove(self)
            .find(hash, &index)
            .map(compact::Found::remove)
            .transpose()
    }

    fn hash(&self, index: usize) -> u64 {
        self.records[index].0
    }
}

pub struct Insert<'a> {
    store: &'a mut Store,
    index: u32,
}

unsafe impl compact::raw::Read for Insert<'_> {
    fn layout(&self) -> compact::Layout {
        self.store.layout
    }
    fn bytes(&self) -> &[u8] {
        tail::View::new(&self.store.bytes).as_slice()
    }
}

unsafe impl compact::raw::Source for Insert<'_> {
    fn hash(value: compact::Value<'_, Self>) -> u64 {
        value.owner().store.hash(value.index())
    }
}

unsafe impl compact::raw::Write for Insert<'_> {
    unsafe fn bytes_mut(&mut self) -> &mut [u8] {
        self.store.bytes.as_mut_slice()
    }
}

unsafe impl compact::raw::Insert for Insert<'_> {
    type Output = usize;
    fn input(&self) -> compact::Value<'_, Self> {
        compact::Value::new(unsafe { depot::raw::Proof::new((self, self.index)) })
    }
    unsafe fn inserted(self) -> usize {
        self.index as usize
    }
}

pub struct Read<'a>(pub &'a Store);

impl Read<'_> {
    pub fn vacant(entry: compact::Entry<Self>) -> Option<Self> {
        match entry {
            compact::Entry::Vacant(owner) => Some(owner),
            compact::Entry::Occupied(_) => None,
        }
    }
}

unsafe impl compact::raw::Read for Read<'_> {
    fn layout(&self) -> compact::Layout {
        self.0.layout
    }
    fn bytes(&self) -> &[u8] {
        tail::View::new(&self.0.bytes).as_slice()
    }
}

unsafe impl compact::raw::Source for Read<'_> {
    fn hash(value: compact::Value<'_, Self>) -> u64 {
        let store = value.owner().0;
        store.hash_reads.set(store.hash_reads.get() + 1);
        store.hash(value.index())
    }
}

unsafe impl compact::raw::Query for Read<'_> {
    type Item = usize;
    fn get(value: compact::Value<'_, Self>) -> usize {
        value.index()
    }
}

pub struct Remove<'a>(pub &'a mut Store);

unsafe impl compact::raw::Read for Remove<'_> {
    fn layout(&self) -> compact::Layout {
        self.0.layout
    }
    fn bytes(&self) -> &[u8] {
        tail::View::new(&self.0.bytes).as_slice()
    }
}

unsafe impl compact::raw::Source for Remove<'_> {
    fn hash(value: compact::Value<'_, Self>) -> u64 {
        value.owner().0.hash(value.index())
    }
}

unsafe impl compact::raw::Query for Remove<'_> {
    type Item = usize;
    fn get(value: compact::Value<'_, Self>) -> usize {
        value.index()
    }
}

unsafe impl compact::raw::Write for Remove<'_> {
    unsafe fn bytes_mut(&mut self) -> &mut [u8] {
        self.0.bytes.as_mut_slice()
    }
}

unsafe impl compact::raw::Remove for Remove<'_> {
    type Output = Result<u64, Error>;
    fn removed(removed: compact::Removed<Self>) -> Self::Output {
        let index = removed.value().index();
        let owner = unsafe { compact::raw::Remove::into_owner(removed) };
        let (hash, live) = &mut owner.0.records[index];
        mem::take(live)
            .then_some(*hash)
            .ok_or(Error::Missing { index })
    }
}
