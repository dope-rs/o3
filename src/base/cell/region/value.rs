use crate::base::cell::{
    local,
    region::{Brand, Identity, Token},
};

/// A value accessible only with the matching state [`Token`].
pub use crate::base::cell::values::Value;

impl<B: Brand, T> Value<B, T> {
    pub fn read<R>(&self, access: local::Read<'_, B>, f: impl for<'a> FnOnce(&'a T) -> R) -> R {
        f(access.region.borrow(self))
    }

    pub fn write<R>(
        &self,
        access: local::Access<'_, B>,
        f: impl for<'a> FnOnce(&'a mut T) -> R,
    ) -> R
    where
        T: Unpin,
    {
        f(access.region.borrow_mut(self))
    }
}

const _: () = {
    assert!(size_of::<Token<Identity<'static>>>() == 0);
    assert!(size_of::<Value<Identity<'static>, u64>>() == size_of::<u64>());
    assert!(align_of::<Value<Identity<'static>, u64>>() == align_of::<u64>());
    assert!(size_of::<(Token<Identity<'static>>, Token<Identity<'static, 1>>)>() == 0);
    assert!(size_of::<Value<Identity<'static, 1>, u64>>() == size_of::<u64>());
    assert!(align_of::<Value<Identity<'static, 1>, u64>>() == align_of::<u64>());
};
