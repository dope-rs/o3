use std::{cell::Cell, marker, mem, pin, ptr};

use o3::collections::{
    pinned::recycle::{self, PoolOwner},
    slab::Capacity,
};

struct Value<'a> {
    address: Cell<*const Self>,
    resets: &'a Cell<usize>,
    value: usize,
    _pin: marker::PhantomPinned,
}

impl Value<'_> {
    fn new(resets: &Cell<usize>) -> Value<'_> {
        Value {
            address: Cell::new(ptr::null()),
            resets,
            value: 0,
            _pin: marker::PhantomPinned,
        }
    }

    fn bind(self: pin::Pin<&Self>) {
        let address = ptr::from_ref(self.get_ref());
        match self.address.get().is_null() {
            true => self.address.set(address),
            false => assert_eq!(self.address.get(), address),
        }
    }

    fn set(self: pin::Pin<&mut Self>, value: usize) {
        // SAFETY: assigning this unpinned field does not move the value.
        unsafe { self.get_unchecked_mut() }.value = value;
    }
}

impl recycle::Recycle for Value<'_> {
    fn recycle(self: pin::Pin<&mut Self>) {
        self.as_ref().bind();
        let this = unsafe { self.get_unchecked_mut() };
        this.resets.set(this.resets.get() + 1);
        this.value = 0;
    }
}

struct Owner<'owner, 'value> {
    pool: ptr::NonNull<recycle::Pool<Value<'value>>>,
    scope: marker::PhantomData<&'owner ()>,
}

// SAFETY: the tests drop every issued handle before the pool and access it
// from one thread only.
unsafe impl<'owner, 'value> PoolOwner<'owner, Value<'value>> for Owner<'owner, 'value> {
    fn pool(self) -> ptr::NonNull<recycle::Pool<Value<'value>>> {
        self.pool
    }
}

fn owner<'owner, 'value>(
    pool: &recycle::Pool<Value<'value>>,
    _scope: &'owner (),
) -> Owner<'owner, 'value> {
    Owner {
        pool: ptr::NonNull::from(pool),
        scope: marker::PhantomData,
    }
}

#[test]
fn values_remain_pinned_and_recycle_in_place() {
    let resets = Cell::new(0);
    let pool = recycle::Pool::with_capacity(Capacity::new(1), |_| Value::new(&resets));
    let mut first = pool.reserve().expect("first slot");
    first.get().bind();
    first.get_mut().set(7);
    let address = ptr::from_ref(first.get().get_ref());
    drop(first);

    assert_eq!(resets.get(), 1);
    let second = pool.reserve().expect("recycled slot");
    second.get().bind();
    assert_eq!(ptr::from_ref(second.get().get_ref()), address);
    assert_eq!(second.get().value, 0);
    drop(second);
    assert_eq!(resets.get(), 2);
}

#[test]
fn reservation_rollback_and_detached_commit_return_the_slot() {
    let resets = Cell::new(0);
    let pool = recycle::Pool::with_capacity(Capacity::new(1), |_| Value::new(&resets));
    drop(pool.reserve().expect("reservation"));
    assert_eq!(resets.get(), 1);

    let scope = ();
    let reservation =
        recycle::Pool::reserve_owned(owner(&pool, &scope)).expect("owned reservation");
    let mut lease = reservation.commit();
    lease.get_mut().set(11);
    let moved = pool;
    assert_eq!(lease.get().value, 11);
    drop(lease);
    assert_eq!(resets.get(), 2);
    assert!(moved.reserve().is_some());
}

#[test]
fn pool_and_handles_preserve_the_pointer_only_layout() {
    type Static = Value<'static>;
    assert_eq!(
        mem::size_of::<recycle::Pool<Static>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<recycle::Reservation<'static, Static>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<recycle::Lease<'static, Static>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<recycle::Lease<'static, Static>>>(),
        mem::size_of::<usize>()
    );
}
