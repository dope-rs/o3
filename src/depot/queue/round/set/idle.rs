use crate::depot::{
    arena::slab::key,
    capacity,
    queue::round::{self, set},
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
        let (source, key) = live.into_inner();
        // SAFETY: the retained idle plane includes the complete occupied source proof.
        let (entry, context): (
            set::Occupied<'a, T, Tag, MAX, true, capacity::Domain<'h>>,
            C,
        ) = unsafe { crate::depot::raw::Proof::new((source, key)) }.into();
        (entry.phase(), context)
    }
}

/// Idle admission retains both the resident identity and the unscheduled owner phase.
type Admission<'a, 'h, T, Tag, const MAX: u32, C, P> = Live<
    (&'a mut round::Set<T, Tag, MAX, capacity::Domain<'h>>, C),
    capacity::Key<'h, key::Handle<Tag, MAX>>,
    P,
>;
