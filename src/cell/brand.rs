use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;

type Invariant<'id> = PhantomData<*mut &'id ()>;

pub struct BrandToken<'id>(Invariant<'id>);

impl BrandToken<'_> {
    pub fn scope<R>(f: impl for<'id> FnOnce(BrandToken<'id>) -> R) -> R {
        f(BrandToken(PhantomData))
    }
}

#[repr(transparent)]
pub struct BrandCell<'id, T> {
    value: UnsafeCell<T>,
    _brand: Invariant<'id>,
}

impl<'id, T> BrandCell<'id, T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            _brand: PhantomData,
        }
    }

    pub fn borrow<'a>(&'a self, token: &'a BrandToken<'id>) -> &'a T {
        let _ = token;
        unsafe { &*self.value.get() }
    }

    pub fn borrow_mut<'a>(&'a self, token: &'a mut BrandToken<'id>) -> &'a mut T
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
        token: &'a mut BrandToken<'id>,
    ) -> Pin<&'a mut T> {
        let _ = token;
        unsafe { Pin::new_unchecked(&mut *self.get_ref().value.get()) }
    }

    pub fn borrow_pin<'a>(self: Pin<&'a Self>, token: &'a BrandToken<'id>) -> Pin<&'a T> {
        unsafe { Pin::new_unchecked(self.get_ref().borrow(token)) }
    }
}
