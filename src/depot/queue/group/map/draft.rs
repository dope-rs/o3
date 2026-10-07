use std::{mem, ops};

use array::init;

use crate::{
    depot::{
        array,
        array::Index,
        queue::{
            group::{
                self, capacity,
                map::{self, Start, mask, path::Path},
            },
            indexed::slab,
        },
    },
    mem::{Lease, Return, lease},
};

pub(in crate::depot::queue::group) struct Draft<'a, 'id, 'b, const LANES: usize, const N: usize> {
    branches: Lease<Undo<'a, 'b, LANES, N>, lease::Open>,
    entries:
        slab::Scope<'a, 'id, map::Entry<LANES>, map::Tag, { u32::MAX }, capacity::Count<LANES>>,
}

enum Added<'id> {
    Root,
    Branch {
        edit: slab::Edit<'id, map::BranchTag>,
        key: u64,
    },
}

struct Undo<'a, 'id, const LANES: usize, const N: usize> {
    scope: slab::Scope<'a, 'id, map::Branch, map::BranchTag, { u32::MAX }, capacity::Count<LANES>>,
    root: map::Root<'a>,
}

impl<'a, 'id, const LANES: usize, const N: usize> Return for Undo<'a, 'id, LANES, N> {
    type Item = [Option<Added<'id>>; N];

    fn return_item(mut self, changes: Self::Item) {
        for change in changes.into_iter().rev().flatten() {
            match change {
                Added::Root => return,
                Added::Branch { edit, key } => {
                    let (parent, branch) = edit.get(&mut self.scope);
                    let target = match parent {
                        Some(parent) => parent.child_mut(key),
                        None => self.root.0,
                    };
                    branch.collapse(key, target);
                }
            }
        }
    }
}

impl<const LANES: usize, const N: usize> Draft<'_, '_, '_, LANES, N> {
    pub(in crate::depot::queue::group) fn with<'a, I, R>(
        map: &'a mut map::Tree<LANES>,
        keys: [(Index<LANES>, u64); N],
        input: I,
        run: impl for<'id> FnOnce(
            &mut slab::Scope<
                'a,
                'id,
                map::Entry<LANES>,
                map::Tag,
                { u32::MAX },
                capacity::Count<LANES>,
            >,
            [slab::Key<'id, map::Tag>; N],
            I,
        ) -> R,
    ) -> Result<R, (group::Error<LANES>, I)> {
        let (map, branches, start) = map.parts_mut();
        slab::Scope::with(map, |mut entries| {
            slab::Scope::with(branches, |branches| {
                let mut storage = mem::MaybeUninit::uninit();
                let slot = match init::Guard::<_, N, false>::new(&mut storage).next() {
                    ops::ControlFlow::Break(full) => {
                        return Ok(run(&mut entries, full.into_array(), input));
                    }
                    ops::ControlFlow::Continue(slot) => slot,
                };
                let (lane, key) = *slot.get(&keys);
                let (mut draft, next) = match start {
                    Start::Empty(root) => {
                        let empty = map::Entry::new(key);
                        let owner = match entries.try_insert(empty) {
                            Ok(owner) => owner,
                            Err(rejected) => {
                                return Err((
                                    group::Error::Owners {
                                        lane,
                                        capacity: *rejected.error(),
                                        key: rejected.value().key,
                                    },
                                    input,
                                ));
                            }
                        };
                        let root = map::Root(
                            root.write(map::Link::Owner(map::Node::inserted(entries.key(owner)))),
                        );
                        let mut changes = [const { None }; N];
                        *slot.get_mut(&mut changes) = Some(Added::Root);
                        (
                            Draft {
                                entries,
                                branches: Lease::open(
                                    Undo {
                                        scope: branches,
                                        root,
                                    },
                                    changes,
                                ),
                            },
                            slot.insert(owner).next(),
                        )
                    }
                    Start::Occupied(root) => (
                        Draft {
                            entries,
                            branches: Lease::open(
                                Undo {
                                    scope: branches,
                                    root,
                                },
                                [const { None }; N],
                            ),
                        },
                        ops::ControlFlow::Continue(slot),
                    ),
                };
                let bound = match next {
                    ops::ControlFlow::Break(full) => full.into_array(),
                    ops::ControlFlow::Continue(mut slot) => loop {
                        let (lane, key) = *slot.get(&keys);
                        let prior = slot
                            .zip(&keys)
                            .find_map(|(bound, prior)| (prior.1 == key).then_some(*bound));
                        let key = match prior {
                            Some(bound) => bound,
                            None => match draft.intern(key, &slot) {
                                Ok(key) => key,
                                Err(cause) => {
                                    return Err((group::Error::owner(lane, cause), input));
                                }
                            },
                        };
                        match slot.insert(key).next() {
                            ops::ControlFlow::Break(full) => break full.into_array(),
                            ops::ControlFlow::Continue(next) => slot = next,
                        }
                    },
                };
                let result = run(&mut draft.entries, bound, input);
                draft.branches.into_parts().0.scope.commit();
                draft.entries.commit();
                Ok(result)
            })
        })
    }
}

impl<'id, const LANES: usize, const N: usize> Draft<'_, 'id, '_, LANES, N> {
    fn intern(
        &mut self,
        key: u64,
        slot: &init::Guard<'_, slab::Key<'id, map::Tag>, N, false, init::Vacant>,
    ) -> Result<slab::Key<'id, map::Tag>, map::Error<LANES>> {
        let (undo, journal) = self.branches.parts_mut();
        let empty = map::Entry::new(key);
        let root = &mut *undo.root.0;
        let mut indices = [mem::MaybeUninit::uninit(); u64::BITS as usize];
        let (existing, path) = match &*root {
            map::Link::Owner(owner) => (owner.bind(&self.entries).into(), None),
            map::Link::Branch(branch) => {
                let mut path = Path::<LANES, _>::new(&mut undo.scope, branch, &mut indices);
                let existing = path.walk(key).bind(&self.entries).into();
                (existing, Some(path))
            }
        };
        let Some(mask) = mask::Mask::between(key, self.entries.get(existing).key) else {
            return Ok(existing);
        };
        let owner = self
            .entries
            .try_insert(empty)
            .map_err(|rejected| map::Error::Owners {
                capacity: *rejected.error(),
                key: rejected.value().key,
            })?;
        let parent = path.and_then(|mut path| path.rewind(mask));
        let edit = undo
            .scope
            .try_splice(parent, |parent, branch| {
                map::Branch::insert(
                    parent,
                    root,
                    mask,
                    map::Node::inserted(branch),
                    map::Node::inserted(self.entries.key(owner)),
                    key,
                )
            })
            .map_err(|slab::Full { capacity }| map::Error::Branches { capacity, key })?;
        *slot.get_mut(journal) = Some(Added::Branch { edit, key });
        Ok(owner)
    }
}
