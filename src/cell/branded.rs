//! GhostCell-style cells with tagged permissions and pin-aware borrowing.
//! Based on <https://plv.mpi-sws.org/rustbelt/ghostcell/>.

use std::{cell::UnsafeCell, marker::PhantomData, pin::Pin};

type Invariant<'id> = PhantomData<*mut &'id ()>;
type Tagged<Tag> = PhantomData<fn(Tag) -> Tag>;

#[doc(hidden)]
pub enum BrandPermission {}

#[doc(hidden)]
pub enum RegionPermission {}

pub struct Token<'id, Tag> {
    _brand: Invariant<'id>,
    _tag: Tagged<Tag>,
}

#[repr(transparent)]
pub struct Branded<'id, T, Tag> {
    value: UnsafeCell<T>,
    _brand: Invariant<'id>,
    _tag: Tagged<Tag>,
}

pub type BrandToken<'id> = Token<'id, BrandPermission>;

pub type Brand<'id, T> = Branded<'id, T, BrandPermission>;

pub type RegionToken<'id> = Token<'id, RegionPermission>;

pub type Region<'id, T> = Branded<'id, T, RegionPermission>;

impl Token<'_, BrandPermission> {
    pub fn scope_with_region<R>(
        f: impl for<'id> FnOnce(BrandToken<'id>, RegionToken<'id>) -> R,
    ) -> R {
        f(Token::new(), Token::new())
    }
}

impl<Tag> Token<'_, Tag> {
    pub fn scope<R>(f: impl for<'id> FnOnce(Token<'id, Tag>) -> R) -> R {
        f(Token::new())
    }

    const fn new() -> Self {
        Self {
            _brand: PhantomData,
            _tag: PhantomData,
        }
    }
}

impl<'id, T, Tag> Branded<'id, T, Tag> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            _brand: PhantomData,
            _tag: PhantomData,
        }
    }

    pub fn borrow<'a>(&'a self, token: &'a Token<'id, Tag>) -> &'a T {
        let _ = token;
        unsafe { &*self.value.get() }
    }

    pub fn borrow_mut<'a>(&'a self, token: &'a mut Token<'id, Tag>) -> &'a mut T
    where
        T: Unpin,
    {
        let _ = token;
        unsafe { &mut *self.value.get() }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }

    pub fn borrow_pin_mut<'a>(
        self: Pin<&'a Self>,
        token: &'a mut Token<'id, Tag>,
    ) -> Pin<&'a mut T> {
        let _ = token;
        unsafe { Pin::new_unchecked(&mut *self.get_ref().value.get()) }
    }

    pub fn borrow_pin<'a>(self: Pin<&'a Self>, token: &'a Token<'id, Tag>) -> Pin<&'a T> {
        unsafe { Pin::new_unchecked(self.get_ref().borrow(token)) }
    }
}
