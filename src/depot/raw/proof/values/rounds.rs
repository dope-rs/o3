use crate::depot::{
    arena::slab::key,
    capacity,
    queue::round::{self, set},
};

// SAFETY: Live retains the occupied source identity and this exact order owner.
unsafe impl<'a, 'h, T, Tag, C, const MAX: u32>
    crate::depot::raw::Project<capacity::Key<'h, key::Handle<Tag, MAX>>>
    for (&'a mut round::Set<T, Tag, MAX, capacity::Domain<'h>>, C)
{
    type Output = (
        set::Occupied<'a, T, Tag, MAX, true, capacity::Domain<'h>>,
        C,
    );

    unsafe fn project(self, key: capacity::Key<'h, key::Handle<Tag, MAX>>) -> Self::Output {
        unsafe { self.0.project(key, self.1) }
    }
}
