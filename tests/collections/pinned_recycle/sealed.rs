use std::{marker, ptr};

use o3::collections::fixed::pinned::recycle::{self, raw};

use crate::pinned_recycle::Value;

pub(super) struct Owner<'owner, 'value> {
    pool: ptr::NonNull<recycle::Pool<Value<'value>>>,
    scope: marker::PhantomData<&'owner ()>,
}

unsafe impl<'owner, 'value> raw::PoolOwner<'owner, Value<'value>> for Owner<'owner, 'value> {
    fn pool(self) -> ptr::NonNull<recycle::Pool<Value<'value>>> {
        self.pool
    }
}

pub(super) fn owner<'owner, 'value>(
    pool: &recycle::Pool<Value<'value>>,
    _scope: &'owner (),
) -> Owner<'owner, 'value> {
    Owner {
        pool: ptr::NonNull::from(pool),
        scope: marker::PhantomData,
    }
}
