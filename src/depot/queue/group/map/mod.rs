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
    Owners {
        capacity: capacity::Count<LANES>,
        key: u64,
    },
    Branches {
        capacity: capacity::Count<LANES>,
        key: u64,
    },
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
pub(super) struct Root<'a>(&'a mut Link);

impl<'a> Root<'a> {
    pub(in crate::depot::queue::group) fn remove<const LANES: usize>(
        self,
        owner: slab::Slot<
            'a,
            Entry<LANES>,
            Tag,
            { u32::MAX },
            links::Hook,
            0,
            capacity::Count<LANES>,
        >,
        branch: Option<slab::Cursor<'a, Branch, BranchTag, { u32::MAX }, capacity::Count<LANES>>>,
    ) {
        let bits = owner.as_ref().key;
        if let Some(branch) = branch {
            branch.remove(|parent, removed| {
                let sibling = removed.sibling(bits);
                match parent {
                    Some(parent) => *parent.child_mut(bits) = sibling,
                    None => *self.0 = sibling,
                }
            });
        }
        owner.detach().discard();
    }
}

enum Start<'a> {
    Empty(&'a mut mem::MaybeUninit<Link>),
    Occupied(Root<'a>),
}

pub(super) struct Bound<'a, const LANES: usize> {
    owner: slab::Slot<'a, Entry<LANES>, Tag, { u32::MAX }, links::Hook, 0, capacity::Count<LANES>>,
    branches: &'a mut Branches<LANES>,
    root: Root<'a>,
    lane: Index<LANES>,
}

enum Position<'a, 'p, const LANES: usize> {
    Root(&'a mut Branches<LANES>),
    Branch(Path<'a, 'p, LANES>),
}

pub(super) struct Rollback<'a, const LANES: usize> {
    added: Option<Added<'a, LANES>>,
}

struct Added<'a, const LANES: usize> {
    branch: Option<slab::Cursor<'a, Branch, BranchTag, { u32::MAX }, capacity::Count<LANES>>>,
    root: Root<'a>,
}

pub(super) struct Found<'a, const LANES: usize> {
    pub(in crate::depot::queue::group) owner:
        slab::Slot<'a, Entry<LANES>, Tag, { u32::MAX }, links::Hook, 0, capacity::Count<LANES>>,
    pub(in crate::depot::queue::group) branch:
        Option<slab::Cursor<'a, Branch, BranchTag, { u32::MAX }, capacity::Count<LANES>>>,
    pub(in crate::depot::queue::group) root: Root<'a>,
}

impl<const LANES: usize> Tree<LANES> {
    fn parts_mut(&mut self) -> (&mut Entries<LANES>, &mut Branches<LANES>, Start<'_>) {
        let root = if self.map.is_empty() {
            Start::Empty(&mut self.root)
        } else {
            // SAFETY: this joint owner initializes its root exactly while map is nonempty.
            Start::Occupied(Root(unsafe { self.root.assume_init_mut() }))
        };
        (&mut self.map, &mut self.branches, root)
    }

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

    pub(in crate::depot::queue::group) fn find(&self, key: u64) -> Option<&Entry<LANES>> {
        if self.map.is_empty() {
            return None;
        }
        // SAFETY: this shared tree borrow retains its nonempty owner slab.
        let mut link = unsafe { self.root.assume_init_ref() };
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
    ) -> Option<Found<'_, LANES>> {
        let (map, branches, Start::Occupied(root)) = self.parts_mut() else {
            return None;
        };
        let (owner, branch) = match &*root.0 {
            Link::Owner(owner) => (owner.bind(&mut *map).into(), None),
            Link::Branch(branch) => {
                let (owner, branch) =
                    path::select(branches, branch, key, |owner| owner.bind(&mut *map).into());
                (owner, Some(branch))
            }
        };
        if owner.as_ref().key != key || lane.get(&owner.as_ref().heads).is_none() {
            return None;
        }
        Some(Found {
            owner,
            branch,
            root,
        })
    }

    pub(in crate::depot::queue::group) fn entry(
        &mut self,
        owner: &Node<Tag>,
        lane: Index<LANES>,
    ) -> Bound<'_, LANES> {
        Bound {
            owner: owner.bind(&mut self.map).into(),
            branches: &mut self.branches,
            // SAFETY: the affine Node and occupied Slot retain an owner
            // in this exact tree, so its root stays initialized until removal.
            root: Root(unsafe { self.root.assume_init_mut() }),
            lane,
        }
    }

    pub(in crate::depot::queue::group) fn intern(
        &mut self,
        key: u64,
    ) -> Result<Lease<Rollback<'_, LANES>, lease::Open>, Error<LANES>> {
        let (map, branches, start) = self.parts_mut();
        let capacity = map.capacity();
        let empty = Entry::new(key);
        let root = match start {
            Start::Empty(root) => {
                let owner = map
                    .vacant()
                    .ok_or(Error::Owners { capacity, key })?
                    .insert(empty);
                let root = Root(root.write(Link::Owner(Node::inserted(owner.key()))));
                return Ok(Lease::open(
                    Rollback {
                        added: Some(Added { branch: None, root }),
                    },
                    owner,
                ));
            }
            Start::Occupied(root) => root.0,
        };
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
            return Ok(Lease::open(Rollback { added: None }, owner));
        };
        let owner = owner
            .into_owner()
            .0
            .vacant()
            .ok_or(Error::Owners { capacity, key })?
            .insert(empty);
        let mut build = |parent: Option<&mut Branch>, branch| {
            Branch::insert(
                parent,
                root,
                mask,
                Node::inserted(branch),
                Node::inserted(owner.key()),
                key,
            )
        };
        let inserted = match position {
            Position::Branch(mut path) => {
                let below = path.rewind(mask).is_some();
                let cursor = path.into_cursor();
                if below {
                    cursor.try_insert(build)
                } else {
                    cursor.try_prepend(build)
                }
            }
            Position::Root(branches) => {
                let capacity = branches.capacity();
                match branches.vacant() {
                    Some(vacant) => Ok(slab::Cursor::new(
                        vacant.insert_with(|branch| build(None, branch)),
                    )),
                    None => Err(capacity),
                }
            }
        };
        let branch = match inserted {
            Ok(branch) => branch,
            Err(capacity) => {
                owner.detach().discard();
                return Err(Error::Branches { capacity, key });
            }
        };
        Ok(Lease::open(
            Rollback {
                added: Some(Added {
                    branch: Some(branch),
                    root: Root(root),
                }),
            },
            owner,
        ))
    }
}

impl<const LANES: usize> Lease<Rollback<'_, LANES>, lease::Open> {
    pub(in crate::depot::queue::group) fn commit(
        self,
        lane: Index<LANES>,
        key: Node<group::EntryTag>,
    ) {
        let (Rollback { added: _ }, mut owner) = self.into_parts();
        *lane.get_mut(&mut owner.as_mut().tails) = Some(key);
    }
}

impl<'a, const LANES: usize> Return for Rollback<'a, LANES> {
    type Item =
        slab::Slot<'a, Entry<LANES>, Tag, { u32::MAX }, links::Hook, 0, capacity::Count<LANES>>;

    fn return_item(self, owner: Self::Item) {
        if let Some(Added { branch, root }) = self.added {
            root.remove(owner, branch);
        }
    }
}

impl<'a, const LANES: usize> Bound<'a, LANES> {
    pub(in crate::depot::queue::group) fn key(&self) -> u64 {
        self.owner.as_ref().key
    }

    pub(in crate::depot::queue::group) fn lane(&self) -> Index<LANES> {
        self.lane
    }

    pub(in crate::depot::queue::group) fn detach(&mut self, next: Option<Node<group::EntryTag>>) {
        self.owner.as_mut().detach(self.lane, next);
    }

    pub(in crate::depot::queue::group) fn restore(
        &mut self,
        key: slab::Handle<group::EntryTag>,
    ) -> Option<Node<group::EntryTag>> {
        let owner = self.owner.as_mut();
        let next = self
            .lane
            .get_mut(&mut owner.heads)
            .replace(Node::inserted(key));
        if next.is_none() {
            *self.lane.get_mut(&mut owner.tails) = Some(Node::inserted(key));
        }
        next
    }

    pub(in crate::depot::queue::group) fn remove(mut self, next: Option<Node<group::EntryTag>>) {
        let lane = self.lane;
        let entry = self.owner.as_ref();
        if next.is_some()
            || entry
                .heads
                .iter()
                .enumerate()
                .any(|(index, head)| index != lane.index() && head.is_some())
        {
            self.owner.as_mut().detach(lane, next);
            return;
        }
        self.last()
    }

    fn last(self) {
        let bits = self.owner.as_ref().key;
        let branch = match &*self.root.0 {
            Link::Owner(_) => None,
            Link::Branch(branch) => Some(path::select(self.branches, branch, bits, |_| ()).1),
        };
        self.root.remove(self.owner, branch);
    }
}

impl<'a, const LANES: usize> Return for Bound<'a, LANES> {
    type Item = (&'a mut u32, Node<Tag>);

    fn return_item(self, _: Self::Item) {
        if self.owner.as_ref().heads.iter().all(Option::is_none) {
            self.last();
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Root<'static>>() == mem::size_of::<&mut Link>());
    assert!(mem::size_of::<Entry<1>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Entry<2>>() == 3 * mem::size_of::<usize>());
};

/// Radix nodes and branches share the same admitted lane capacity.
type Entries<const N: usize> =
    slab::Slab<Entry<N>, Tag, { u32::MAX }, links::Hook, 0, capacity::Count<N>>;
type Branches<const N: usize> =
    slab::Slab<Branch, BranchTag, { u32::MAX }, links::Hook, 0, capacity::Count<N>>;
