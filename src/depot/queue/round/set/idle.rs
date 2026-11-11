use crate::depot::{
    arena::slab::{self, key},
    capacity,
    queue::{
        links,
        round::{self, set},
    },
    raw::Live,
};

/// An occupied resident proved absent from its owner's schedule.
#[derive(Debug)]
pub struct Idle;

impl<'a, T, Tag, const MAX: u32, const ROOT: bool, B>
    set::Occupied<'a, T, Tag, MAX, ROOT, B, Idle>
{
    pub fn insert(self) -> set::Occupied<'a, T, Tag, MAX, ROOT, B, set::Ready> {
        let entry = self;
        // SAFETY: this capability exclusively owns a vacant hook in this exact order.
        unsafe { entry.order.ring().push(entry.order, entry.index) };
        entry.phase()
    }

    pub fn discard(self) {
        self.entry.discard();
    }
}

impl<'a, 'h, T, Tag, const MAX: u32>
    set::Occupied<'a, T, Tag, MAX, true, capacity::Domain<'h>, Idle>
{
    /// Consumes this exact resident's occupied, unscheduled source plane.
    pub fn from_live<C>(live: Admission<'a, 'h, T, Tag, MAX, C, Self>) -> (Self, C) {
        let ((source, context), key) = live.into_inner();
        // SAFETY: this admitted key retains an occupied resident in the same domain.
        let index = unsafe { links::Index::new_usize(key.as_inner().index() as usize) };
        let live = unsafe { crate::depot::raw::Proof::new(((&mut source.values, context), index)) };
        let (entry, context) = slab::Occupied::from_live(live);
        (Self::new(entry, &source.order), context)
    }
}

/// Idle admission retains both the resident identity and the unscheduled owner phase.
type Admission<'a, 'h, T, Tag, const MAX: u32, C, P> = Live<
    (&'a mut round::Set<T, Tag, MAX, capacity::Domain<'h>>, C),
    capacity::Key<'h, key::Handle<Tag, MAX>>,
    P,
>;
