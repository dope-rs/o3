use std::marker;

use crate::depot::{
    index::Initialized,
    queue::{indexed::keyed, links},
};

/// One FIFO member retaining its exclusive slot owner.
pub struct Slot<O: crate::depot::raw::KeyOwner<P>, P = (), const LINKED: bool = true> {
    owner: O,
    plane: marker::PhantomData<fn(P) -> P>,
}

impl<
    O: crate::depot::raw::KeyOwner<
            P,
            Fifo = keyed::Fifo<<O as crate::depot::raw::KeyOwner<P>>::Key, P>,
        >,
    P,
> Slot<O, P>
{
    pub fn try_new(owner: O) -> Result<Self, Slot<O, P, false>> {
        let entry = owner.slot();
        if entry.link().raw.previous.get() == entry.index().raw() {
            // SAFETY: the owner admits only canonical vacancy or membership in its FIFO.
            return Err(Slot::from_source(unsafe {
                crate::depot::raw::Proof::new(owner)
            }));
        }
        Ok(Self {
            owner,
            plane: marker::PhantomData,
        })
    }

    pub fn front<Q: crate::depot::raw::KeyPlane<P, Slot = O>>(owner: Q) -> Option<Self> {
        let slot = Q::select(keyed::Fifo::first(owner)?);
        Some(Self {
            owner: slot,
            plane: marker::PhantomData,
        })
    }

    pub fn key(&self) -> O::Key {
        self.owner.slot().key()
    }

    pub fn successor(
        &self,
    ) -> Option<crate::depot::raw::Proof<(&O, links::Index), Initialized<P>>> {
        let index = self.owner.slot().successor()?;
        // SAFETY: membership retains this neighbor's initialization in the same owner and plane.
        Some(unsafe { crate::depot::raw::Proof::new((&self.owner, index)) })
    }

    /// Transfers the original owner into its successor, retaining the tail otherwise.
    pub fn advance(
        self,
    ) -> Result<crate::depot::raw::Proof<(O, links::Index), Initialized<P>>, Self> {
        let Some(index) = self.owner.slot().successor() else {
            return Err(self);
        };
        // SAFETY: the exclusive owner retains both linked slots in the same plane.
        Ok(unsafe { crate::depot::raw::Proof::new((self.owner, index)) })
    }

    pub fn move_to_back(self) -> Self {
        if self.owner.fifo().raw.last() == Some(self.owner.slot().index()) {
            return self;
        }
        self.remove().push::<false>()
    }

    pub fn remove(self) -> Slot<O, P, false> {
        let entry = self.owner.slot();
        let before = entry.predecessor();
        let after = entry.successor();
        let previous = before.map(|index| {
            // SAFETY: this member retains its live predecessor in this owner.
            O::entry(unsafe { crate::depot::raw::Proof::new((&self.owner, index)) })
        });
        let next = after.map(|index| {
            // SAFETY: this member retains its live successor in this owner.
            O::entry(unsafe { crate::depot::raw::Proof::new((&self.owner, index)) })
        });
        let edge = links::Edge::new(
            entry.index(),
            links::Index::encode(before),
            links::Index::encode(after),
        );
        // SAFETY: the retained owner supplies this exact plane and membership phase.
        let change = unsafe {
            crate::depot::raw::Detach::resolved(
                &self.owner.fifo().raw,
                edge,
                &entry.link().raw,
                previous.map(|entry| &entry.link().raw),
                next.map(|entry| &entry.link().raw),
            )
        };

        crate::depot::raw::Removal::commit(change).idle();
        // SAFETY: the consuming detach committed vacancy in this exact owner.
        Slot::from_source(unsafe { crate::depot::raw::Proof::new(self.owner) })
    }
}

impl<
    O: crate::depot::raw::KeyRoute<
            P,
            Fifo = keyed::Fifo<<O as crate::depot::raw::KeyOwner<P>>::Key, P>,
        >,
    P,
> Slot<O, P>
{
    pub fn transfer(self) -> Slot<O::Target, P> {
        self.remove().route().push::<false>()
    }
}

impl<
    O: crate::depot::raw::KeyFinish<LINKED, P>
        + crate::depot::raw::KeyOwner<
            P,
            Fifo = keyed::Fifo<<O as crate::depot::raw::KeyOwner<P>>::Key, P>,
        >,
    P,
    const LINKED: bool,
> Slot<O, P, LINKED>
{
    pub fn release(self) -> O::Output {
        // SAFETY: the original owner and its exact membership phase are consumed together.
        unsafe { <O as crate::depot::raw::KeyFinish<LINKED, P>>::finish(self.owner) }
    }
}

impl<
    O: crate::depot::raw::KeyOwner<
            P,
            Fifo = keyed::Fifo<<O as crate::depot::raw::KeyOwner<P>>::Key, P>,
        >,
    P,
> Slot<O, P, false>
{
    pub fn try_vacant(owner: O) -> Result<Self, Slot<O, P>> {
        match Slot::<O, P>::try_new(owner) {
            Ok(linked) => Err(linked),
            Err(vacant) => Ok(vacant),
        }
    }

    pub fn push<const FRONT: bool>(self) -> Slot<O, P> {
        let entry = self.owner.slot();
        let fifo = &self.owner.fifo().raw;
        let end = if FRONT { fifo.first() } else { fifo.last() };
        let adjacent = end.map(|index| {
            // SAFETY: the owner's FIFO retains this initialized endpoint.
            O::entry(unsafe { crate::depot::raw::Proof::new((&self.owner, index)) })
        });
        let index = links::Index::encode(end);
        let key = entry.index();
        let edge = if FRONT {
            links::Edge::new(key, u32::MAX, index)
        } else {
            links::Edge::new(key, index, u32::MAX)
        };
        // SAFETY: the retained owner supplies this exact plane and membership phase.
        let change = unsafe {
            crate::depot::raw::Insert::<(), FRONT>::resolved(
                fifo,
                edge,
                &entry.link().raw,
                adjacent.map(|entry| &entry.link().raw),
            )
        };

        crate::depot::raw::Insert::commit(change);
        // SAFETY: the consuming insertion established membership in this FIFO.
        Slot::from_source(unsafe { crate::depot::raw::Proof::new(self.owner) })
    }
}

impl<
    O: crate::depot::raw::KeyRoute<
            P,
            Fifo = keyed::Fifo<<O as crate::depot::raw::KeyOwner<P>>::Key, P>,
        >,
    P,
> Slot<O, P, false>
{
    pub fn route(self) -> Slot<O::Target, P, false> {
        // SAFETY: Route moves the same detached slot and root authority to its destination.
        Slot::from_source(unsafe { crate::depot::raw::Proof::new(self.owner.route()) })
    }
}

impl<O: crate::depot::raw::KeyOwner<P>, P, const LINKED: bool> Slot<O, P, LINKED> {
    pub fn from_source(source: crate::depot::raw::Proof<O, keyed::Membership<LINKED>>) -> Self {
        Self {
            owner: source.into_inner(),
            plane: marker::PhantomData,
        }
    }
}

impl<O: crate::depot::raw::KeyOwner<P>, P> Slot<O, P> {
    pub fn owner_mut(source: crate::depot::raw::Proof<&mut Self, keyed::Exclusive>) -> &mut O {
        &mut source.into_inner().owner
    }
}
