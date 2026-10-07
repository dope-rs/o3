use std::{
    cell::{Cell, RefCell},
    marker, pin, ptr,
    rc::Rc,
};

use o3::depot::arena::pool::{recycle, recycle::release};

pub struct Value<'a> {
    pub address: Cell<*const Self>,
    pub resets: &'a Cell<Option<usize>>,
    pub value: Cell<usize>,
    pub pin: marker::PhantomPinned,
}

pub struct Releases<'a> {
    pub closed: &'a Cell<Option<usize>>,
    pub released: &'a Cell<(Option<usize>, bool)>,
}

type Callback = Box<dyn Fn()>;

pub struct Reentry {
    pub callback: Rc<RefCell<Option<Callback>>>,
    pub closed: Rc<Cell<Option<usize>>>,
    pub entered: Rc<Cell<bool>>,
    pub status: Rc<Cell<Result<(), Error>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unready,
    Unentered,
    Borrowed,
    Missing,
}

pub struct Reused;

#[derive(Default)]
pub struct Pinned {
    pub value: Cell<usize>,
    pub pin: marker::PhantomPinned,
}

impl Drop for Releases<'_> {
    fn drop(&mut self) {
        self.closed
            .set(self.closed.get().and_then(|count| count.checked_add(1)));
    }
}

impl<'owner, 'a: 'owner> release::Context<'owner, Value<'a>> for Releases<'a> {
    fn released(&self, stable: bool) {
        let (count, valid) = self.released.get();
        self.released.set((
            count.and_then(|count| count.checked_add(1)),
            valid && stable,
        ));
    }
}

impl Drop for Reentry {
    fn drop(&mut self) {
        self.closed
            .set(self.closed.get().and_then(|count| count.checked_add(1)));
        self.status.set(
            self.status
                .get()
                .and(self.entered.get().then_some(()).ok_or(Error::Unentered)),
        );
    }
}

impl<'owner> release::Context<'owner, Reused> for Reentry {
    fn released(&self, ready: bool) {
        let status = if !ready {
            Err(Error::Unready)
        } else if self.entered.replace(true) {
            Ok(())
        } else {
            match self.callback.try_borrow() {
                Ok(callback) => callback.as_ref().ok_or(Error::Missing).map(|call| call()),
                Err(_) => Err(Error::Borrowed),
            }
        };
        self.status.set(self.status.get().and(status));
    }
}

impl recycle::Recycle for Reused {
    type Seed = ();
    type Release = bool;

    fn from_seed((): Self::Seed) -> Self {
        Self
    }

    fn recycle(self: pin::Pin<&mut Self>) -> bool {
        true
    }
}

impl Value<'_> {
    pub fn bind(self: pin::Pin<&Self>) -> (*const Self, *const Self) {
        let addr = ptr::from_ref(self.get_ref());
        if self.address.get().is_null() {
            self.address.set(addr);
        }
        (self.address.get(), addr)
    }

    pub fn set(self: pin::Pin<&mut Self>, value: usize) {
        self.as_ref().get_ref().value.set(value);
    }
}

impl<'a> recycle::Recycle for Value<'a> {
    type Seed = &'a Cell<Option<usize>>;
    type Release = bool;

    fn from_seed(resets: Self::Seed) -> Self {
        Self {
            address: Cell::new(ptr::null()),
            resets,
            value: Cell::new(0),
            pin: marker::PhantomPinned,
        }
    }

    fn recycle(self: pin::Pin<&mut Self>) -> bool {
        let (stored, current) = self.as_ref().bind();
        let this = self.as_ref().get_ref();
        this.resets
            .set(this.resets.get().and_then(|count| count.checked_add(1)));
        this.value.set(0);
        stored == current
    }
}
