use std::future::Future;
use std::marker::PhantomData;
use std::mem::{self, MaybeUninit};
use std::pin::Pin;
use std::task::{Context, Poll};

const ALIGN: usize = 16;

#[repr(C, align(16))]
struct Storage<const SIZE: usize> {
    bytes: [u8; SIZE],
}

pub struct InlineFuture<'a, Out: 'a, const SIZE: usize = 2048> {
    storage: MaybeUninit<Storage<SIZE>>,
    poll_fn: unsafe fn(*mut u8, &mut Context<'_>) -> Poll<Out>,
    drop_fn: unsafe fn(*mut u8),
    try_fn: Option<unsafe fn(*mut u8) -> Out>,
    _marker: PhantomData<&'a ()>,
}

impl<'a, Out: 'a, const SIZE: usize> InlineFuture<'a, Out, SIZE> {
    pub fn new<F>(fut: F) -> Self
    where
        F: Future<Output = Out> + 'a,
    {
        assert_inline_fits::<F, SIZE>();
        Self {
            storage: init_storage(fut),
            poll_fn: poll_impl::<F, Out>,
            drop_fn: drop_impl::<F>,
            try_fn: None,
            _marker: PhantomData,
        }
    }

    pub unsafe fn from_raw<F: 'a>(
        fut: F,
        poll_fn: unsafe fn(*mut u8, &mut Context<'_>) -> Poll<Out>,
        drop_fn: unsafe fn(*mut u8),
    ) -> Self {
        assert_inline_fits::<F, SIZE>();
        Self {
            storage: init_storage(fut),
            poll_fn,
            drop_fn,
            try_fn: None,
            _marker: PhantomData,
        }
    }

    pub fn with_try_fn(mut self, try_fn: unsafe fn(*mut u8) -> Out) -> Self {
        self.try_fn = Some(try_fn);
        self
    }

    pub fn try_now(self) -> std::result::Result<Out, Self> {
        let Some(try_fn) = self.try_fn else {
            return Err(self);
        };
        let mut this = mem::ManuallyDrop::new(self);
        Ok(unsafe { try_fn(this.storage.as_mut_ptr().cast::<u8>()) })
    }

    #[inline(always)]
    pub unsafe fn poll_pinned(&mut self, cx: &mut Context<'_>) -> Poll<Out> {
        unsafe { Pin::new_unchecked(self).poll(cx) }
    }
}

impl<'a, Out: 'a, const SIZE: usize> Future for InlineFuture<'a, Out, SIZE> {
    type Output = Out;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let this = self.get_unchecked_mut();
            (this.poll_fn)(this.storage.as_mut_ptr().cast::<u8>(), cx)
        }
    }
}

impl<'a, Out: 'a, const SIZE: usize> Drop for InlineFuture<'a, Out, SIZE> {
    fn drop(&mut self) {
        unsafe { (self.drop_fn)(self.storage.as_mut_ptr().cast::<u8>()) };
    }
}

fn assert_inline_fits<F, const SIZE: usize>() {
    assert!(
        mem::size_of::<F>() <= SIZE,
        "InlineFuture: future size {} exceeds inline storage {} for {}",
        mem::size_of::<F>(),
        SIZE,
        std::any::type_name::<F>()
    );
    assert!(
        mem::align_of::<F>() <= ALIGN,
        "InlineFuture: future align {} exceeds inline align {}",
        mem::align_of::<F>(),
        ALIGN
    );
}

fn init_storage<F, const SIZE: usize>(fut: F) -> MaybeUninit<Storage<SIZE>> {
    let mut storage = MaybeUninit::<Storage<SIZE>>::uninit();
    let raw = storage.as_mut_ptr().cast::<u8>().cast::<F>();
    unsafe { raw.write(fut) };
    storage
}

unsafe fn poll_impl<F, Out>(raw: *mut u8, cx: &mut Context<'_>) -> Poll<Out>
where
    F: Future<Output = Out>,
{
    let pin = unsafe { Pin::new_unchecked(&mut *(raw as *mut F)) };
    pin.poll(cx)
}

unsafe fn drop_impl<F>(raw: *mut u8) {
    unsafe { std::ptr::drop_in_place(raw as *mut F) };
}
