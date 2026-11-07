use std::marker;

use crate::depot::{
    index::Initialized,
    queue::{indexed::keyed, links},
    raw,
};

/// One FIFO member retaining its exclusive slot owner.
#[repr(transparent)]
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
        let (index, link) = keyed::selected::<_, P>(&owner);
        if link.raw.previous.get() == index.raw() {
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
        self.owner.parts().0
    }

    pub fn successor(
        &self,
    ) -> Option<crate::depot::raw::Proof<(&O, links::Index), Initialized<P>>> {
        let index = self.owner.parts().1.raw.next_index()?;
        // SAFETY: membership retains this neighbor's initialization in the same owner and plane.
        Some(unsafe { crate::depot::raw::Proof::new((&self.owner, index)) })
    }

    /// Transfers the original owner into its successor, retaining the tail otherwise.
    pub fn advance(
        self,
    ) -> Result<crate::depot::raw::Proof<(O, links::Index), Initialized<P>>, Self> {
        let Some(index) = self.owner.parts().1.raw.next_index() else {
            return Err(self);
        };
        // SAFETY: the exclusive owner retains both linked slots in the same plane.
        Ok(unsafe { crate::depot::raw::Proof::new((self.owner, index)) })
    }

    pub fn move_to_back(self) -> Self {
        if links::Index::encode(self.owner.fifo().raw.last())
            == keyed::selected::<_, P>(&self.owner).0.raw()
        {
            return self;
        }
        self.remove().push::<false>()
    }

    pub fn remove(self) -> Slot<O, P, false> {
        let (index, link) = keyed::selected::<_, P>(&self.owner);
        let before = link.raw.prev_index();
        let after = link.raw.next_index();
        let resolve = |index: Option<links::Index>| {
            index.map(|index| {
                // SAFETY: this member retains its live neighbor in the same owner and plane.
                let link = O::link(unsafe { raw::Proof::new((&self.owner, index)) });
                raw::EndpointHook::from(&link.raw)
            })
        };
        let previous = resolve(before);
        let next = resolve(after);
        let fifo = &self.owner.fifo().raw;
        let neighbor = |index, hook| match hook {
            Some(hook) => raw::Neighbor::node(links::Index::encode(index), hook),
            None => raw::Neighbor::head(links::Index::END, fifo),
        };
        // SAFETY: both neighbors retain this exact member until reciprocal repair and vacancy.
        unsafe {
            raw::EndpointHook::from(&link.raw).detach(
                index,
                neighbor(before, previous),
                neighbor(after, next),
            )
        };
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
        let (index, link) = keyed::selected::<_, P>(&self.owner);
        let fifo = &self.owner.fifo().raw;
        let end = if FRONT { fifo.first() } else { fifo.last() };
        let adjacent = end.map(|index| {
            // SAFETY: the owner's FIFO retains this initialized endpoint.
            let link = O::link(unsafe { raw::Proof::new((&self.owner, index)) });
            (index, raw::EndpointHook::from(&link.raw))
        });
        // SAFETY: the retained owner supplies this exact plane and membership phase.
        unsafe {
            raw::EndpointHook::from(&link.raw).push::<FRONT, _>(
                fifo,
                index,
                links::Index::END,
                adjacent,
            )
        };
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
