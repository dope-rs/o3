use std::cell::Cell;
use std::cmp::Ordering;
use std::marker::PhantomPinned;
use std::pin::Pin;

pub(crate) struct PinnedItem<'a> {
    value: Cell<u32>,
    address: Cell<*const Self>,
    drops: &'a Cell<usize>,
    _pin: PhantomPinned,
}

impl<'a> PinnedItem<'a> {
    pub(crate) fn new(value: u32, drops: &'a Cell<usize>) -> Self {
        Self {
            value: Cell::new(value),
            address: Cell::new(std::ptr::null()),
            drops,
            _pin: PhantomPinned,
        }
    }

    pub(crate) fn bind(self: Pin<&Self>) {
        self.address.set(std::ptr::from_ref(self.get_ref()));
    }

    pub(crate) fn set(self: Pin<&mut Self>, value: u32) {
        self.as_ref().value.set(value);
    }

    pub(crate) fn value(self: Pin<&Self>) -> u32 {
        self.value.get()
    }
}

impl Drop for PinnedItem<'_> {
    fn drop(&mut self) {
        if !self.address.get().is_null() {
            assert_eq!(self.address.get(), std::ptr::from_ref(self));
        }
        self.drops.set(self.drops.get() + 1);
    }
}

pub(crate) struct PanicDrop<'a> {
    order: u8,
    drops: &'a Cell<usize>,
    panic_once: &'a Cell<bool>,
}

impl<'a> PanicDrop<'a> {
    pub(crate) fn new(order: u8, drops: &'a Cell<usize>, panic_once: &'a Cell<bool>) -> Self {
        Self {
            order,
            drops,
            panic_once,
        }
    }
}

impl Drop for PanicDrop<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if self.panic_once.replace(false) {
            panic!("drop panic");
        }
    }
}

impl PartialEq for PanicDrop<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order
    }
}

impl Eq for PanicDrop<'_> {}

impl PartialOrd for PanicDrop<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PanicDrop<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.order.cmp(&other.order)
    }
}
