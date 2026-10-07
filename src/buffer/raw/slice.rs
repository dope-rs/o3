use std::ops;

use crate::{
    buffer,
    buffer::{bytes::scope, storage::range},
};
/// One exclusive target paired with its original owner's selected range.
pub struct Slice<'a, 'id, S: ?Sized> {
    target: &'a mut S,
    range: scope::Range<'id>,
}

impl<'a, S: buffer::raw::Store> Slice<'a, '_, S> {
    /// Selections cannot escape the exclusive operation.
    pub fn try_with<T>(
        target: &'a mut S,
        bounds: ops::Range<usize>,
        run: impl for<'id> FnOnce(Slice<'a, 'id, S>) -> T,
    ) -> Result<T, range::Error> {
        scope::Owner::with(target, |owner| {
            let range = owner.borrow().try_get(bounds)?;
            Ok(run(Slice {
                target: owner.into_inner(),
                range,
            }))
        })
    }
}

impl<S: buffer::raw::Store> Slice<'_, '_, S> {
    pub(in crate::buffer) fn try_project(
        mut target: S,
        bounds: ops::Range<usize>,
    ) -> Result<S, crate::mem::Rejected<S, range::Error>> {
        match Slice::try_with(&mut target, bounds, S::project) {
            Ok(()) => Ok(target),
            Err(error) => Err(crate::mem::Rejected::new(target, error)),
        }
    }
}

impl<'a, 'id, S: ?Sized> Slice<'a, 'id, S> {
    /// # Safety
    /// `target` must expose the original operation's unchanged visible bytes and
    /// preserve its original owner.
    pub unsafe fn from_parts(target: &'a mut S, range: scope::Range<'id>) -> Self {
        Self { target, range }
    }

    pub fn into_parts(self) -> (&'a mut S, scope::Range<'id>) {
        (self.target, self.range)
    }

    /// # Safety
    /// The projected target must expose the original target's unchanged visible
    /// bytes and preserve its original owner.
    pub unsafe fn map<T: ?Sized>(
        self,
        project: impl FnOnce(&'a mut S) -> &'a mut T,
    ) -> Slice<'a, 'id, T> {
        Slice {
            target: project(self.target),
            range: self.range,
        }
    }
}

impl<S: buffer::raw::Store> Slice<'_, '_, S> {
    pub(in crate::buffer::raw) fn apply(self) {
        if self.range.is_empty()
            && let Some(empty) = S::empty()
        {
            *self.target = empty;
            return;
        }
        S::restrict(self);
    }
}

const _: () = {
    assert!(size_of::<Slice<'static, 'static, Vec<u8>>>() == 3 * size_of::<usize>());
    assert!(
        size_of::<Option<Slice<'static, 'static, Vec<u8>>>>()
            == size_of::<Slice<'static, 'static, Vec<u8>>>()
    );
};
