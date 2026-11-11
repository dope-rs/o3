use std::mem;

pub(in crate::depot::queue::group) use branch::Branch;
pub(in crate::depot::queue::group) use draft::Draft;
pub(in crate::depot::queue::group) use node::Node;
use path::Path;

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
    mem::{Lease, Return, lease},
};

mod branch;
mod draft;
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

#[repr(C)]
pub(super) struct Bound<'a, const LANES: usize, P = ()> {
    tree: &'a mut Tree<LANES>,
    owner: links::Index,
    phase: P,
}

#[repr(C)]
pub(super) struct Change {
    owner: links::Index,
    branch: Option<Place>,
}

pub(super) type Place = (links::Index, Option<links::Index>);
pub(super) type Rollback<'a, const N: usize> = Bound<'a, N, Option<Change>>;
pub(super) type Added<'a, const N: usize> = Bound<'a, N, Option<Place>>;

enum Position<'a, 'p, const LANES: usize> {
    Root(&'a mut Branches<LANES>),
    Branch(Path<'a, 'p, LANES>),
}

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
    ) -> Option<Lease<Added<'_, LANES>, lease::Open>> {
        let (owner, phase) = match self.root()? {
            Link::Owner(owner) => (owner, None),
            Link::Branch(branch) => {
                let (owner, branch) = path::select(&self.branches, branch, key);
                (owner, Some(branch))
            }
        };
        let owner = owner.bind(&self.map).key().0;
        let bound = Bound {
            tree: self,
            owner,
            phase,
        };
        let entry = bound.get();
        if entry.key != key || lane.get(&entry.heads).is_none() {
            return None;
        }
        Some(Lease::open(bound, ()))
    }

    pub(in crate::depot::queue::group) fn entry(&mut self, owner: &Node<Tag>) -> Bound<'_, LANES> {
        let owner = owner.bind(&self.map).key().0;
        Bound {
            tree: self,
            owner,
            phase: (),
        }
    }

    pub(in crate::depot::queue::group) fn intern(
        &mut self,
        key: u64,
    ) -> Result<Lease<Rollback<'_, LANES>, lease::Open>, Error<LANES>> {
        let mut phase = None;
        let (root, owner) = self.start(key, &mut phase)?;
        let owner = match owner {
            Some(owner) => owner,
            None => root.intern(key, &mut phase)?,
        };
        let owner = owner.bind(&self.map).key().0;
        Ok(Lease::open(
            Bound {
                tree: self,
                owner,
                phase,
            },
            (),
        ))
    }

    #[inline(always)]
    fn start(
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

    fn remove(&mut self, change: Change, key: u64) {
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

impl<const LANES: usize> Root<'_, LANES> {
    #[inline(always)]
    fn intern(self, key: u64, change: &mut Option<Change>) -> Result<Node<Tag>, Error<LANES>> {
        let Tree {
            map,
            branches,
            root,
        } = self.0;
        // SAFETY: Root is constructed only while its exclusive tree has a live owner.
        let root = unsafe { root.assume_init_mut() };
        let capacity = map.capacity();
        let mut indices = [mem::MaybeUninit::uninit(); u64::BITS as usize];
        let (owner, position) = match root {
            Link::Owner(owner) => (owner.bind(&mut *map).into(), Position::Root(branches)),
            Link::Branch(branch) => {
                let mut path = Path::new(branches, branch, &mut indices);
                let owner = path.walk(key).bind(&mut *map).into();
                (owner, Position::Branch(path))
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
        let (branches, parent) = match position {
            Position::Root(branches) => (branches, None),
            Position::Branch(mut path) => {
                let parent = path.rewind(mask);
                (path.source, parent)
            }
        };
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

impl<const LANES: usize, P> Bound<'_, LANES, P> {
    pub(in crate::depot::queue::group) fn get(&self) -> &Entry<LANES> {
        // SAFETY: this bound owner remains live in its retained exclusive tree.
        unsafe { crate::depot::raw::Proof::new((&self.tree.map, self.owner)) }.into()
    }

    pub(in crate::depot::queue::group) fn get_mut(&mut self) -> &mut Entry<LANES> {
        // SAFETY: this reborrow retains the admitted owner and excludes all other access.
        let slot: slab::Slot<
            '_,
            Entry<LANES>,
            Tag,
            { u32::MAX },
            links::Hook,
            0,
            capacity::Count<LANES>,
        > = unsafe { crate::depot::raw::Proof::new((&mut self.tree.map, self.owner)) }.into();
        slot.into_mut()
    }
}

impl<const LANES: usize> Return for Rollback<'_, LANES> {
    type Item = ();

    fn return_item(self, (): ()) {
        if let Some(change) = self.phase {
            let owner = Bound {
                tree: self.tree,
                owner: self.owner,
                phase: (),
            };
            let key = owner.get().key;
            owner.tree.remove(change, key);
        }
    }
}

impl<const LANES: usize> Return for Added<'_, LANES> {
    type Item = ();

    fn return_item(self, (): ()) {
        let entry = self.get();
        if entry.heads.iter().all(Option::is_none) {
            let key = entry.key;
            self.tree.remove(
                Change {
                    owner: self.owner,
                    branch: self.phase,
                },
                key,
            );
        }
    }
}

impl<const LANES: usize> Bound<'_, LANES> {
    pub(in crate::depot::queue::group) fn key(&self) -> u64 {
        self.get().key
    }

    pub(in crate::depot::queue::group) fn detach(
        &mut self,
        lane: Index<LANES>,
        next: Option<Node<group::EntryTag>>,
    ) {
        self.get_mut().detach(lane, next);
    }

    pub(in crate::depot::queue::group) fn restore(
        mut self,
        lane: Index<LANES>,
        key: slab::Handle<group::EntryTag>,
    ) -> (Node<Tag>, Option<Node<group::EntryTag>>) {
        let owner_key = self.node();
        let owner = self.get_mut();
        let next = lane.get_mut(&mut owner.heads).replace(Node::inserted(key));
        if next.is_none() {
            *lane.get_mut(&mut owner.tails) = Some(Node::inserted(key));
        }
        (owner_key, next)
    }

    pub(in crate::depot::queue::group) fn remove(
        mut self,
        lane: Index<LANES>,
        next: Option<Node<group::EntryTag>>,
    ) {
        if next.is_some()
            || self
                .get()
                .heads
                .iter()
                .enumerate()
                .any(|(index, head)| index != lane.index() && head.is_some())
        {
            self.detach(lane, next);
            return;
        }
        self.last()
    }

    fn last(self) {
        let key = self.get().key;
        // SAFETY: Bound retains an admitted owner, so the root remains initialized.
        let branch = match unsafe { self.tree.root.assume_init_ref() } {
            Link::Owner(_) => None,
            Link::Branch(branch) => Some(path::select(&self.tree.branches, branch, key).1),
        };
        self.tree.remove(
            Change {
                owner: self.owner,
                branch,
            },
            key,
        );
    }
}

impl<'a, const LANES: usize> Return for Bound<'a, LANES> {
    type Item = &'a mut u32;

    fn return_item(self, _: Self::Item) {
        if self.get().heads.iter().all(Option::is_none) {
            self.last();
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Root<'static, 1>>() == mem::size_of::<&mut Link>());
    assert!(mem::size_of::<Entry<1>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Entry<2>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Place>>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Change>>() == 3 * mem::size_of::<u32>());
    assert!(mem::size_of::<Bound<'static, 1>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<Rollback<'static, 1>>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<Added<'static, 1>>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Result<Node<Tag>, Error<1>>>() <= 2 * mem::size_of::<usize>());
};

/// Radix nodes and branches share the same admitted lane capacity.
type Entries<const N: usize> =
    slab::Slab<Entry<N>, Tag, { u32::MAX }, links::Hook, 0, capacity::Count<N>>;
type Branches<const N: usize> =
    slab::Slab<Branch, BranchTag, { u32::MAX }, links::Hook, 0, capacity::Count<N>>;
