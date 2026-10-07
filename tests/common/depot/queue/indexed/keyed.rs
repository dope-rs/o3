use o3::{
    alloc::{self, BoxSliceExt as _},
    depot,
    depot::{
        arena::slab::key,
        capacity,
        index::Initialized,
        queue::{
            indexed::keyed::{self, lanes},
            links,
        },
    },
};

pub struct Node {
    pub key: key::Handle,
    link: keyed::Link<key::Handle>,
    pub value: u64,
}

// SAFETY: every native resident binds its admitted key to its own initialized link.
unsafe impl<'a> depot::raw::KeyBorrow<'a> for &'a Node {
    type Key = key::Handle;
    fn parts(self) -> (Self::Key, &'a keyed::Link<key::Handle>) {
        (self.key, &self.link)
    }
}

pub struct Lanes {
    heads: lanes::Fifo<key::Handle, 2>,
    slots: Box<[Node]>,
    pub checks: Option<usize>,
}

pub struct Entry<'a> {
    owner: &'a mut Lanes,
    index: usize,
}

#[derive(Debug)]
pub enum Error {
    Bounds {
        requested: key::Handle,
        capacity: usize,
    },
    Stale {
        requested: key::Handle,

        current: key::Handle,
    },

    Linked(key::Handle),

    Detached(key::Handle),
}

impl Lanes {
    pub fn try_new(capacity: u32) -> Result<Self, alloc::Error> {
        Ok(Self {
            heads: lanes::Fifo::try_new(capacity::Count::new(capacity))?,
            slots: Box::try_from_fn(capacity as usize, |index| {
                // SAFETY: the admitted lane capacity excludes both endpoint tags.
                let index = unsafe { <links::Index as depot::raw::Word>::from_offset(index) };
                let key =
                    key::Handle::from_parts(key::Parts::new(index.raw(), key::Generation::MIN));
                Node {
                    key,
                    link: keyed::Link::from(index),
                    value: u64::from(index.raw()),
                }
            })?,
            checks: Some(0),
        })
    }

    pub fn as_slice(&self) -> &[Node] {
        &self.slots
    }

    pub fn resolve(&mut self, key: key::Handle) -> Result<Entry<'_>, Error> {
        self.checks = self.checks.and_then(|count| count.checked_add(1));
        let index = key.index() as usize;
        let node = self.slots.get(index).ok_or(Error::Bounds {
            requested: key,
            capacity: self.slots.len(),
        })?;
        if node.key != key {
            return Err(Error::Stale {
                requested: key,
                current: node.key,
            });
        }
        Ok(Entry { owner: self, index })
    }

    fn vacant_entry(&mut self, key: key::Handle) -> Result<Entry<'_>, Error> {
        let entry = self.resolve(key)?;
        if depot::raw::KeyOwner::parts(&entry).1.is_linked(key) {
            return Err(Error::Linked(key));
        }
        Ok(entry)
    }

    pub fn vacant(
        &mut self,
        key: key::Handle,
    ) -> Result<lanes::Bound<Entry<'_>, 2, (), false>, Error> {
        let entry = self.vacant_entry(key)?;
        Ok(lanes::Bound::from_source(unsafe {
            depot::raw::Proof::new(entry)
        }))
    }

    pub fn try_replace(
        &mut self,
        key: key::Handle,
        tag: key::Generation,
        value: u64,
    ) -> Result<key::Handle, Error> {
        let entry = self.vacant_entry(key)?;
        // SAFETY: the vacancy retains this exact checked index and exclusive root.
        let node = unsafe { entry.owner.slots.get_unchecked_mut(entry.index) };
        node.key = key::Handle::from_parts(key::Parts::new(key.index(), tag));
        node.value = value;
        Ok(node.key)
    }

    pub fn member(&mut self, key: key::Handle) -> Result<lanes::Bound<Entry<'_>, 2>, Error> {
        let entry = self.resolve(key)?;
        if !depot::raw::KeyOwner::parts(&entry).1.is_linked(key) {
            return Err(Error::Detached(key));
        }
        Ok(lanes::Bound::from_source(unsafe {
            depot::raw::Proof::new(entry)
        }))
    }
}

unsafe impl<'a, const L: usize> depot::raw::LaneSelect<2, L> for &'a mut Lanes {
    type Key = key::Handle;
    type Slot = Entry<'a>;
}

impl AsRef<lanes::Fifo<key::Handle, 2>> for Lanes {
    fn as_ref(&self) -> &lanes::Fifo<key::Handle, 2> {
        &self.heads
    }
}

unsafe impl<'a> depot::raw::Project<links::Index> for &'a mut Lanes {
    type Output = Entry<'a>;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        Entry {
            owner: self,
            index: index.get(),
        }
    }
}

unsafe impl depot::raw::KeyOwner for Entry<'_> {
    type Key = key::Handle;
    type Fifo = lanes::Fifo<Self::Key, 2>;
    fn fifo(&self) -> &lanes::Fifo<Self::Key, 2> {
        &self.owner.heads
    }
    fn parts(&self) -> (Self::Key, &keyed::Link<Self::Key>) {
        let node = unsafe { self.owner.slots.get_unchecked(self.index) };
        depot::raw::KeyBorrow::parts(node)
    }
    fn entry<'a>(
        live: depot::raw::Proof<(&'a Self, links::Index), Initialized<()>>,
    ) -> keyed::Entry<'a, Self::Key> {
        let owner = *live.source();
        let index = live.key();
        let node = unsafe { owner.owner.slots.get_unchecked(index.get()) };
        keyed::Entry::from_source(node)
    }
}

impl lanes::Access for Entry<'_> {
    type Read<'a>
        = &'a u64
    where
        Self: 'a;
    type Edit<'a>
        = &'a mut u64
    where
        Self: 'a;

    fn get(&mut self) -> Self::Read<'_> {
        &unsafe { self.owner.slots.get_unchecked(self.index) }.value
    }
    fn edit(&mut self) -> Self::Edit<'_> {
        &mut unsafe { self.owner.slots.get_unchecked_mut(self.index) }.value
    }
}

unsafe impl depot::raw::KeyFinish<false> for Entry<'_> {
    type Output = key::Handle;
    unsafe fn finish(self) -> Self::Output {
        unsafe { self.owner.slots.get_unchecked(self.index) }.key
    }
}
