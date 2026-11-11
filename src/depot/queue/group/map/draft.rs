use std::{marker, mem, ops};

use crate::{
    depot::{
        array::{self, Index},
        capacity,
        queue::group::{self, map},
        raw::Live,
    },
    mem::{Lease, Return},
};

pub(in crate::depot::queue::group) struct Draft<'a, 'id, const LANES: usize> {
    tree: &'a mut map::Tree<LANES>,
    brand: marker::PhantomData<capacity::Domain<'id>>,
}

pub(super) type Key<'id> = capacity::Key<'id, map::node::Key<map::Tag>>;

struct Undo<'a, 'id, 'k, const LANES: usize, const N: usize> {
    draft: Draft<'a, 'id, LANES>,
    keys: &'k [(Index<LANES>, u64); N],
}

impl<const LANES: usize, const N: usize> Return for Undo<'_, '_, '_, LANES, N> {
    type Item = [Option<map::Change>; N];

    fn return_item(self, changes: Self::Item) {
        for (change, (_, key)) in changes.into_iter().rev().zip(self.keys.iter().rev()) {
            if let Some(change) = change {
                self.draft.tree.remove(change, *key);
            }
        }
    }
}

impl<'a, const LANES: usize> Draft<'a, '_, LANES> {
    pub(in crate::depot::queue::group) fn with<I, const N: usize>(
        tree: &'a mut map::Tree<LANES>,
        keys: [(Index<LANES>, u64); N],
        input: I,
        run: impl for<'id> FnOnce(&mut Draft<'a, 'id, LANES>, [Key<'id>; N], I),
    ) -> Result<(), (group::Error<LANES>, I)> {
        let mut draft = Self {
            tree,
            brand: marker::PhantomData,
        };
        let mut storage = mem::MaybeUninit::uninit();
        let slot = match array::init::Guard::<_, N, false>::new(&mut storage).next() {
            ops::ControlFlow::Break(full) => {
                run(&mut draft, full.into_array(), input);
                return Ok(());
            }
            ops::ControlFlow::Continue(slot) => slot,
        };
        let mut pending = Lease::open(Undo { draft, keys: &keys }, [const { None }; N]);
        let (undo, journal) = pending.parts_mut();
        let (lane, key) = *slot.get(&keys);
        let (root, owner) = match undo.draft.tree.start(key, slot.get_mut(journal)) {
            Ok(start) => start,
            Err(cause) => {
                pending.into_parts();
                return Err((group::Error::owner(lane, key, cause), input));
            }
        };
        let next = match owner {
            Some(owner) => slot.insert(Self::bind(owner.bind(&root.0.map))).next(),
            None => ops::ControlFlow::Continue(slot),
        };
        let bound = match next {
            ops::ControlFlow::Break(full) => full.into_array(),
            ops::ControlFlow::Continue(mut slot) => loop {
                let (lane, key) = *slot.get(&keys);
                let prior = slot
                    .zip(&keys)
                    .find_map(|(bound, prior)| (prior.1 == key).then_some(*bound));
                let owner = match prior {
                    Some(owner) => owner,
                    None => match map::Root(&mut *root.0).intern(key, slot.get_mut(journal)) {
                        Ok(owner) => Self::bind(owner.bind(&root.0.map)),
                        Err(cause) => {
                            if slot.as_slice().is_empty() {
                                pending.into_parts();
                            }
                            return Err((group::Error::owner(lane, key, cause), input));
                        }
                    },
                };
                match slot.insert(owner).next() {
                    ops::ControlFlow::Break(full) => break full.into_array(),
                    ops::ControlFlow::Continue(next) => slot = next,
                }
            },
        };
        let (mut undo, _) = pending.into_parts();
        run(&mut undo.draft, bound, input);
        Ok(())
    }
}

impl<'id, const LANES: usize> Draft<'_, 'id, LANES> {
    fn bind(owner: Live<&map::Entries<LANES>, &map::node::Key<map::Tag>>) -> Key<'id> {
        // SAFETY: only with mints this brand, from a live owner in its exact retained tree.
        // Insertion preserves the fixed capacity and every admitted owner until publication ends.
        capacity::Key::new(unsafe { crate::depot::raw::Proof::new(**owner.key()) })
    }

    pub(in crate::depot::queue::group) fn get_mut(
        &mut self,
        key: Key<'id>,
    ) -> &mut map::Entry<LANES> {
        let owner = self.member(key);
        self.tree.get_mut(&owner)
    }
}
