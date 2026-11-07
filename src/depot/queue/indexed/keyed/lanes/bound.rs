use std::marker;

use crate::depot::queue::{
    indexed::keyed::{
        self,
        lanes::{self, fifo},
    },
    links,
};

/// Exact exclusive slot authority with its membership phase retained.
#[repr(transparent)]
pub struct Bound<
    S: crate::depot::raw::KeyOwner<
            P,
            Fifo = lanes::Fifo<<S as crate::depot::raw::KeyOwner<P>>::Key, N, P>,
        >,
    const N: usize,
    P = (),
    const LINKED: bool = true,
> {
    pub(super) slot: S,
    plane: marker::PhantomData<fn(P) -> P>,
}

impl<
    S: crate::depot::raw::KeyOwner<
            P,
            Fifo = lanes::Fifo<<S as crate::depot::raw::KeyOwner<P>>::Key, N, P>,
        >,
    const N: usize,
    P,
    const LINKED: bool,
> Bound<S, N, P, LINKED>
{
    pub fn from_source(source: crate::depot::raw::Proof<S, keyed::Membership<LINKED>>) -> Self {
        Self {
            slot: source.into_inner(),
            plane: marker::PhantomData,
        }
    }

    pub(super) fn append<const L: usize>(&self) {
        const { assert!(L < N) };
        let (pos, link) = keyed::selected::<_, P>(&self.slot);
        let heads = &self.slot.fifo().heads[L];
        let adjacent = heads.last().map(|tail| {
            // SAFETY: this exact lane retains its live tail and original root.
            let previous = S::link(unsafe { crate::depot::raw::Proof::new((&self.slot, tail)) });
            (tail, crate::depot::raw::EndpointHook::from(&previous.raw))
        });
        // SAFETY: this exact vacant slot and selected lane retain the same root and plane.
        unsafe {
            crate::depot::raw::EndpointHook::from(&link.raw).push::<false, _>(
                heads,
                pos,
                fifo::endpoint(L),
                adjacent,
            )
        };
    }

    fn detach_links(&self) {
        let (index, link) = keyed::selected::<_, P>(&self.slot);
        let hook = &link.raw;
        let previous = hook.previous.get();
        let next = hook.next.get();
        // SAFETY: membership retains this exact compact slot and both reciprocal neighbors.
        unsafe {
            crate::depot::raw::EndpointHook::from(hook).detach(
                index,
                self.neighbor(previous),
                self.neighbor(next),
            )
        };
    }

    fn neighbor(&self, raw: u32) -> crate::depot::raw::Neighbor<'_, u32> {
        use crate::depot::raw;

        let lane = (u32::MAX - raw) as usize;
        match self.slot.fifo().heads.get(lane) {
            Some(head) => raw::Neighbor::head(raw, head),
            None => {
                // SAFETY: a linked non-endpoint neighbor is live in this exact owner and plane.
                let link = S::link(unsafe {
                    crate::depot::raw::Proof::new((&self.slot, links::Index::new_raw(raw)))
                });
                raw::Neighbor::node(raw, raw::EndpointHook::from(&link.raw))
            }
        }
    }

    pub fn key(&self) -> S::Key {
        self.slot.parts().0
    }
}

impl<
    S: crate::depot::raw::KeyOwner<
            P,
            Fifo = lanes::Fifo<<S as crate::depot::raw::KeyOwner<P>>::Key, N, P>,
        >,
    const N: usize,
    P,
> Bound<S, N, P>
{
    pub fn front<const L: usize, A: crate::depot::raw::LaneSelect<N, L, P, Slot = S>>(
        owner: A,
    ) -> Option<Self>
    where
        S: From<A::Output>,
    {
        const { assert!(L < N) };
        let head = owner.as_ref().heads[L].first()?;
        let live = unsafe { crate::depot::raw::Proof::new((owner, head)) };
        Some(Self::from_source(unsafe {
            crate::depot::raw::Proof::new(live.into().into())
        }))
    }

    pub fn detach(self) -> Bound<S, N, P, false> {
        self.detach_links();
        Bound::from_source(unsafe { crate::depot::raw::Proof::new(self.slot) })
    }

    pub fn move_to_back<const L: usize>(&mut self) {
        const { assert!(L < N) };
        let (pos, _) = keyed::selected::<_, P>(&self.slot);
        if links::Index::encode(self.slot.fifo().heads[L].last()) == pos.raw() {
            return;
        }
        self.detach_links();
        self.append::<L>();
    }

    pub fn get(&mut self) -> S::Read<'_>
    where
        S: lanes::Access<P>,
    {
        self.slot.get()
    }
}

impl<
    S: crate::depot::raw::KeyOwner<
            P,
            Fifo = lanes::Fifo<<S as crate::depot::raw::KeyOwner<P>>::Key, N, P>,
        >,
    const N: usize,
    P,
> Bound<S, N, P, false>
{
    pub fn push_back<const L: usize>(self) -> Bound<S, N, P> {
        self.append::<L>();
        Bound::from_source(unsafe { crate::depot::raw::Proof::new(self.slot) })
    }

    pub fn edit(&mut self) -> S::Edit<'_>
    where
        S: lanes::Access<P>,
    {
        self.slot.edit()
    }

    pub fn take(self) -> S::Output
    where
        S: crate::depot::raw::KeyFinish<false, P>,
    {
        unsafe { <S as crate::depot::raw::KeyFinish<false, P>>::finish(self.slot) }
    }
}
