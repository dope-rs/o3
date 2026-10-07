use std::{marker, mem, ptr};

use crate::depot::{
    self,
    arena::linked::{self, fifo, fifo::ends, slots},
};

/// An initialized detached slot and the owner that may publish it.
#[must_use = "an initialized slot must be published or released"]
pub struct Live<T, H: linked::Source<T>, Tag = ()> {
    pub(super) vacant: slots::Vacant<T, H, Tag>,
}

impl<T, H: linked::Source<T>, Tag> Live<T, H, Tag> {
    /// # Safety
    /// The slot was detached from this owner's topology exactly once.
    pub(crate) unsafe fn detached(owner: H, index: slots::Index<Tag>) -> Self {
        Self {
            vacant: slots::Vacant {
                owner,
                index,
                value: marker::PhantomData,
            },
        }
    }

    pub fn get(&self) -> &T {
        unsafe { self.vacant.owner.store().value(self.vacant.index) }
    }

    pub(crate) fn take(self) -> (T, slots::Vacant<T, H, Tag>) {
        let this = mem::ManuallyDrop::new(self);
        let node = unsafe {
            this.vacant
                .owner
                .store()
                .region
                .slot(this.vacant.index.raw())
        };
        let value = unsafe { (&*node.value.get()).assume_init_read() };
        node.next.set(this.vacant.index.raw());
        (value, unsafe { ptr::read(&this.vacant) })
    }

    /// Destroys the detached value and retains its exact slot reservation.
    pub(crate) fn clear(self) -> slots::Vacant<T, H, Tag> {
        let this = mem::ManuallyDrop::new(self);
        let index = this.vacant.index.raw();
        let node = unsafe { this.vacant.owner.store().region.slot(index) };
        // SAFETY: this consumed Live owns this node's initialized value.
        unsafe { node.clear(index) };
        // SAFETY: destruction consumed the initialized value; the sole vacant
        // entitlement transfers unchanged without running Live::drop again.
        unsafe { ptr::read(&this.vacant) }
    }

    pub(crate) const fn index(&self) -> slots::Index<Tag> {
        self.vacant.index
    }

    /// The owner's exact store and this slot's entitlement must remain unchanged.
    pub(crate) unsafe fn owner_mut(&mut self) -> &mut H {
        &mut self.vacant.owner
    }

    /// # Safety
    /// The exact slot has been attached to this owner's topology.
    pub(crate) unsafe fn publish(self) -> H {
        let this = mem::ManuallyDrop::new(self);
        unsafe { ptr::read(&this.vacant.owner) }
    }
}

impl<T, H: linked::Source<T>, Tag> Drop for Live<T, H, Tag> {
    fn drop(&mut self) {
        let index = self.vacant.index.raw();
        let node = unsafe { self.vacant.owner.store().region.slot(index) };
        // SAFETY: Drop consumes this node's sole initialized-value owner.
        unsafe { node.clear(index) };
    }
}

impl<T, Tag> Live<T, &slots::Store<T>, Tag> {
    /// # Safety
    /// The links are a live partition in this claim's exact store.
    pub(crate) unsafe fn append(self, links: &mut ends::Exclusive<slots::Index<Tag>>) {
        let mut claim = self;
        let index = claim.index();
        unsafe { links.append_store(claim.owner_mut(), index) };
        unsafe { claim.publish() };
    }

    /// # Safety
    /// The links are a live partition in this claim's exact store.
    pub(crate) unsafe fn prepend(self, links: &mut ends::Exclusive<slots::Index<Tag>>) {
        let mut claim = self;
        let index = claim.index();
        unsafe { links.prepend_link(claim.owner_mut(), index) };
        unsafe { claim.publish() };
    }

    /// Publishes the initialized claim into the FIFO proved to retain its exact store.
    pub fn push_back(proof: depot::raw::Proof<(Self, &mut fifo::Fifo<Tag>), linked::Same>) {
        let (claim, state) = proof.into_inner();
        unsafe { state.state.append(claim) };
    }
}
