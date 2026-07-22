use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;

type Invariant<'id> = PhantomData<*mut &'id ()>;
type Tagged<Tag> = PhantomData<fn(Tag) -> Tag>;

#[doc(hidden)]
pub enum BrandPermission {}

#[doc(hidden)]
pub enum RegionPermission {}

pub struct BrandedToken<'id, Tag> {
    _brand: Invariant<'id>,
    _tag: Tagged<Tag>,
}

#[repr(transparent)]
pub struct BrandedCell<'id, T, Tag> {
    value: UnsafeCell<T>,
    _brand: Invariant<'id>,
    _tag: Tagged<Tag>,
}

pub type BrandToken<'id> = BrandedToken<'id, BrandPermission>;

pub type BrandCell<'id, T> = BrandedCell<'id, T, BrandPermission>;

pub type RegionToken<'id> = BrandedToken<'id, RegionPermission>;

pub type RegionCell<'id, T> = BrandedCell<'id, T, RegionPermission>;

impl BrandedToken<'_, BrandPermission> {
    pub fn scope_with_region<R>(
        f: impl for<'id> FnOnce(BrandToken<'id>, RegionToken<'id>) -> R,
    ) -> R {
        f(BrandedToken::new(), BrandedToken::new())
    }
}

impl<Tag> BrandedToken<'_, Tag> {
    pub fn scope<R>(f: impl for<'id> FnOnce(BrandedToken<'id, Tag>) -> R) -> R {
        f(BrandedToken::new())
    }

    const fn new() -> Self {
        Self {
            _brand: PhantomData,
            _tag: PhantomData,
        }
    }
}

impl<'id, T, Tag> BrandedCell<'id, T, Tag> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            _brand: PhantomData,
            _tag: PhantomData,
        }
    }

    pub fn borrow<'a>(&'a self, token: &'a BrandedToken<'id, Tag>) -> &'a T {
        let _ = token;
        unsafe { &*self.value.get() }
    }

    pub fn borrow_mut<'a>(&'a self, token: &'a mut BrandedToken<'id, Tag>) -> &'a mut T
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
        token: &'a mut BrandedToken<'id, Tag>,
    ) -> Pin<&'a mut T> {
        let _ = token;
        unsafe { Pin::new_unchecked(&mut *self.get_ref().value.get()) }
    }

    pub fn borrow_pin<'a>(self: Pin<&'a Self>, token: &'a BrandedToken<'id, Tag>) -> Pin<&'a T> {
        unsafe { Pin::new_unchecked(self.get_ref().borrow(token)) }
    }
}
