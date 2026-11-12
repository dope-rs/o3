use std::{mem, num::NonZero};

pub(in crate::depot::queue::group) use branch::Branch;
pub(in crate::depot::queue::group) use node::Node;

use crate::{
    alloc,
    depot::{
        array::Index,
        queue::{
            group::{self, capacity},
            indexed::slab,
            links,
        },
    },
};

mod branch;
mod mask;
mod node;
mod path;

pub(super) enum Tag {}
pub(super) enum BranchTag {}

pub(super) enum Link {
    Owner(Node<Tag>),
    Branch(Node<BranchTag>),
}

pub(super) enum Error<const LANES: usize> {
    Owners(capacity::Count<LANES>),
    Branches(capacity::Count<LANES>),
}

pub(super) struct Entry<const LANES: usize> {
    pub(in crate::depot::queue::group) key: u64,
    pub(in crate::depot::queue::group) heads: [Option<Node<group::EntryTag>>; LANES],
    pub(in crate::depot::queue::group) tails: [Option<Node<group::EntryTag>>; LANES],
}

impl<const LANES: usize> Entry<LANES> {
    fn new(key: u64) -> Self {
        Self {
            key,
            heads: [const { None }; LANES],
            tails: [const { None }; LANES],
        }
    }

    pub(in crate::depot::queue::group) fn detach(
        &mut self,
        lane: Index<LANES>,
        next: Option<Node<group::EntryTag>>,
    ) {
        *lane.get_mut(&mut self.heads) = next;
        if lane.get(&self.heads).is_none() {
            *lane.get_mut(&mut self.tails) = None;
        }
    }
}

pub(super) struct Tree<const LANES: usize> {
    map: Entries<LANES>,
    branches: Branches<LANES>,
    root: mem::MaybeUninit<Link>,
}

#[repr(transparent)]
pub(super) struct Root<'a, const LANES: usize>(&'a mut Tree<LANES>);

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct Change {
    owner: links::Index,
    branch: Option<Place>,
}

pub(super) type Place = (links::Index, Option<links::Index>);

impl<const LANES: usize> Tree<LANES> {
    pub(in crate::depot::queue::group) fn clear(&mut self) {
        while self.map.pop_front().is_some() {}
        while self.branches.pop_front().is_some() {}
    }

    pub(in crate::depot::queue::group) const fn record_bytes() -> usize {
        mem::size_of::<Entry<LANES>>()
            + mem::size_of::<slab::Handle<Tag>>()
            + mem::size_of::<links::Hook>()
            + mem::size_of::<Branch>()
            + mem::size_of::<slab::Handle<BranchTag>>()
            + mem::size_of::<links::Hook>()
    }

    pub(in crate::depot::queue::group) fn try_new(
        capacity: capacity::Count<LANES>,
    ) -> Result<Self, alloc::Error> {
        Ok(Self {
            map: slab::Slab::try_from(capacity)?,
            branches: slab::Slab::try_from(capacity)?,
            root: mem::MaybeUninit::uninit(),
        })
    }

    fn root(&self) -> Option<&Link> {
        if self.map.is_empty() {
            return None;
        }
        // SAFETY: this joint owner initializes its root exactly while map is nonempty.
        Some(unsafe { self.root.assume_init_ref() })
    }

    pub(in crate::depot::queue::group) fn find(&self, key: u64) -> Option<&Entry<LANES>> {
        let mut link = self.root()?;
        loop {
            match link {
                Link::Owner(owner) => {
                    let entry: &Entry<LANES> = owner.bind(&self.map).into();
                    return (entry.key == key).then_some(entry);
                }
                Link::Branch(branch) => {
                    let branch: &Branch = branch.bind(&self.branches).into();
                    link = branch.child(key);
                }
            }
        }
    }

    pub(in crate::depot::queue::group) fn select(
        &mut self,
        key: u64,
        lane: Index<LANES>,
    ) -> Option<(Node<Tag>, Option<Place>)> {
        let (owner, phase) = match self.root()? {
            Link::Owner(owner) => (owner, None),
            Link::Branch(branch) => {
                let (owner, branch) = path::select(&self.branches, branch, key);
                (owner, Some(branch))
            }
        };
        let entry = self.get(owner);
        if entry.key != key || lane.get(&entry.heads).is_none() {
            return None;
        }
        Some((Node::new(owner.index()), phase))
    }

    pub(in crate::depot::queue::group) fn get(&self, owner: &Node<Tag>) -> &Entry<LANES> {
        owner.bind(&self.map).into()
    }

    pub(in crate::depot::queue::group) fn get_mut(
        &mut self,
        owner: &Node<Tag>,
    ) -> &mut Entry<LANES> {
        owner.bind(&mut self.map).into().into_mut()
    }

    pub(in crate::depot::queue::group) fn intern(
        &mut self,
        key: u64,
        change: &mut Option<Change>,
    ) -> Result<Node<Tag>, Error<LANES>> {
        let (mut root, owner) = self.start(key, change)?;
        match owner {
            Some(owner) => Ok(owner),
            None => root.intern(key, change),
        }
    }

    #[inline(always)]
    pub(super) fn start(
        &mut self,
        key: u64,
        change: &mut Option<Change>,
    ) -> Result<(Root<'_, LANES>, Option<Node<Tag>>), Error<LANES>> {
        let owner = if self.map.is_empty() {
            let capacity = self.map.capacity();
            let owner = self
                .map
                .vacant()
                .ok_or(Error::Owners(capacity))?
                .insert(Entry::new(key));
            let handle = owner.key();
            self.root.write(Link::Owner(Node::inserted(handle)));
            *change = Some(Change {
                owner: owner.index,
                branch: None,
            });
            Some(Node::inserted(handle))
        } else {
            None
        };
        Ok((Root(self), owner))
    }

    pub(super) fn remove(&mut self, change: Change, key: u64) {
        if let Some(place) = change.branch {
            // SAFETY: the consumed change retains its distinct initialized branch and parent;
            // later insertions are removed first, before this exact exclusive tree is reclaimed.
            let branch: slab::Cursor<'_, Branch, BranchTag, { u32::MAX }, capacity::Count<LANES>> =
                unsafe { crate::depot::raw::Proof::new((&mut self.branches, place)) }.into();
            // SAFETY: a retained branch keeps this tree's root initialized until unlink completes.
            let root = unsafe { self.root.assume_init_mut() };
            branch.remove(|parent, removed| {
                let sibling = removed.sibling(key);
                match parent {
                    Some(parent) => *parent.child_mut(key) = sibling,
                    None => *root = sibling,
                }
            });
        }
        // SAFETY: the consumed change owns this exact live owner, now unlinked from its branch.
        let owner: slab::Slot<
            '_,
            Entry<LANES>,
            Tag,
            { u32::MAX },
            links::Hook,
            0,
            capacity::Count<LANES>,
        > = unsafe { crate::depot::raw::Proof::new((&mut self.map, change.owner)) }.into();
        owner.detach().discard();
    }
}

impl<'a, const LANES: usize> Root<'a, LANES> {
    pub(super) fn into_inner(self) -> &'a mut Tree<LANES> {
        self.0
    }

    #[inline(always)]
    pub(super) fn intern(
        &mut self,
        key: u64,
        change: &mut Option<Change>,
    ) -> Result<Node<Tag>, Error<LANES>> {
        let Tree {
            map,
            branches,
            root,
        } = &mut *self.0;
        // SAFETY: Root is constructed only while its exclusive tree has a live owner.
        let root = unsafe { root.assume_init_mut() };
        let capacity = map.capacity();
        let mut indices = [mem::MaybeUninit::uninit(); u64::BITS as usize];
        let (owner, length) = match root {
            Link::Owner(owner) => (owner.bind(&mut *map).into(), None),
            Link::Branch(branch) => {
                let mut current = branch.bind(&*branches).key().0;
                indices[0].write(current);
                let mut len = NonZero::<usize>::MIN;
                let owner = loop {
                    // SAFETY: every stored identity was bound in this unchanged source.
                    let branch: &Branch =
                        unsafe { crate::depot::raw::Proof::new((&*branches, current)) }.into();
                    let branch = match branch.child(key) {
                        Link::Owner(owner) => break owner.bind(&mut *map).into(),
                        Link::Branch(branch) => branch,
                    };
                    current = branch.bind(&*branches).key().0;
                    // SAFETY: decreasing one-bit masks bound the prefix to 64 entries,
                    // and its incremented length remains nonzero.
                    unsafe {
                        indices.get_unchecked_mut(len.get()).write(current);
                        len = NonZero::new_unchecked(len.get() + 1);
                    }
                };
                (owner, Some(len))
            }
        };
        let Some(mask) = mask::Mask::between(key, owner.as_ref().key) else {
            return Ok(Node::inserted(owner.key()));
        };
        let owner = owner
            .into_owner()
            .0
            .vacant()
            .ok_or(Error::Owners(capacity))?
            .insert(Entry::new(key));
        let mut parent = None;
        if let Some(mut position) = length {
            loop {
                // SAFETY: this position retains an initialized index in this exact path.
                let index = unsafe { indices.get_unchecked(position.get() - 1).assume_init_read() };
                // SAFETY: the path retains this live branch under the exclusive tree loan.
                let branch: &Branch =
                    unsafe { crate::depot::raw::Proof::new((&*branches, index)) }.into();
                if branch.mask > mask {
                    parent = Some(index);
                    break;
                }
                let Some(next) = NonZero::new(position.get() - 1) else {
                    break;
                };
                position = next;
            }
        }
        let capacity = branches.capacity();
        let Some(vacant) = branches.vacant() else {
            owner.detach().discard();
            return Err(Error::Branches(capacity));
        };
        let handle = owner.key();
        // SAFETY: the retained decreasing-mask path proves a live parent distinct from this vacancy.
        let mut branch = unsafe { slab::Cursor::new(vacant, parent) };
        let added = Node::inserted(branch.key());
        let value = Branch::insert(
            branch.parent_mut(),
            root,
            mask,
            added,
            Node::inserted(handle),
            key,
        );
        let branch = branch.into_inner().insert(value);
        *change = Some(Change {
            owner: owner.index,
            branch: Some((branch.index, parent)),
        });
        Ok(Node::inserted(handle))
    }
}

impl<const LANES: usize> Tree<LANES> {
    pub(in crate::depot::queue::group) fn rollback(&mut self, change: Change) {
        let key = self.get(&Node::new(change.owner)).key;
        self.remove(change, key);
    }

    pub(in crate::depot::queue::group) fn restore(
        &mut self,
        owner: Node<Tag>,
        lane: Index<LANES>,
        key: slab::Handle<group::EntryTag>,
    ) -> (Node<Tag>, Option<Node<group::EntryTag>>) {
        let entry = self.get_mut(&owner);
        let next = lane.get_mut(&mut entry.heads).replace(Node::inserted(key));
        if next.is_none() {
            *lane.get_mut(&mut entry.tails) = Some(Node::inserted(key));
        }
        (owner, next)
    }

    pub(in crate::depot::queue::group) fn remove_member(
        &mut self,
        owner: Node<Tag>,
        lane: Index<LANES>,
        next: Option<Node<group::EntryTag>>,
    ) {
        let entry = self.get(&owner);
        if next.is_some()
            || entry
                .heads
                .iter()
                .enumerate()
                .any(|(index, head)| index != lane.index() && head.is_some())
        {
            self.get_mut(&owner).detach(lane, next);
            return;
        }
        self.last(&owner);
    }

    pub(in crate::depot::queue::group) fn prune(&mut self, owner: &Node<Tag>) {
        if self.get(owner).heads.iter().all(Option::is_none) {
            self.last(owner);
        }
    }

    pub(in crate::depot::queue::group) fn prune_at(
        &mut self,
        owner: &Node<Tag>,
        branch: Option<Place>,
    ) {
        let entry = self.get(owner);
        if entry.heads.iter().all(Option::is_none) {
            let key = entry.key;
            self.remove(
                Change {
                    owner: owner.index(),
                    branch,
                },
                key,
            );
        }
    }

    fn last(&mut self, owner: &Node<Tag>) {
        let key = self.get(owner).key;
        // SAFETY: the retained owner keeps this tree's root initialized.
        let branch = match unsafe { self.root.assume_init_ref() } {
            Link::Owner(_) => None,
            Link::Branch(branch) => Some(path::select(&self.branches, branch, key).1),
        };
        self.remove(
            Change {
                owner: owner.index(),
                branch,
            },
            key,
        );
    }
}

const _: () = {
    assert!(mem::size_of::<Root<'static, 1>>() == mem::size_of::<&mut Link>());
    assert!(mem::size_of::<Entry<1>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Entry<2>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Place>>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Change>>() == 3 * mem::size_of::<u32>());
    assert!(mem::size_of::<Result<Node<Tag>, Error<1>>>() <= 2 * mem::size_of::<usize>());
};

/// Radix nodes and branches share the same admitted lane capacity.
type Entries<const N: usize> =
    slab::Slab<Entry<N>, Tag, { u32::MAX }, links::Hook, 0, capacity::Count<N>>;
type Branches<const N: usize> =
    slab::Slab<Branch, BranchTag, { u32::MAX }, links::Hook, 0, capacity::Count<N>>;
