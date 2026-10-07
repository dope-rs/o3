use std::{marker, mem, num, ptr};

pub use probe::Probe;
pub(crate) use probe::Spare;
pub(in crate::depot::map) use probe::{Candidate, Directory, Shift, State};

use crate::{
    alloc,
    depot::{capacity, map},
};
mod probe;

pub(in crate::depot::map) fn scan<D: Directory, E, const SPARE: bool>(
    owner: &D,
    hash: u64,
    matches: impl Fn(map::raw::Occupied) -> Result<bool, E>,
) -> Result<State, E> {
    if SPARE {
        if owner.buckets() == 0 {
            return Ok(State::Full);
        }
        let mut scan = probe::Scan::<_, true>::new(owner, hash);
        try_match(matches, || match scan.next() {
            map::Entry::Occupied(bucket) => State::Occupied(bucket),
            map::Entry::Vacant(bucket) => State::Vacant(bucket),
        })
    } else {
        let mut scan = probe::Scan::<_, false>::new(owner, hash);
        try_match(matches, || scan.next())
    }
}

fn try_match<E>(
    matches: impl Fn(map::raw::Occupied) -> Result<bool, E>,
    mut next: impl FnMut() -> State,
) -> Result<State, E> {
    loop {
        let bucket = match next() {
            State::Occupied(bucket) => bucket,
            end => return Ok(end),
        };
        if matches(bucket)? {
            return Ok(State::Occupied(bucket));
        }
    }
}

/// # Safety
/// This exact directory retains an occupied bucket admitted by matches and hash.
pub(in crate::depot::map) unsafe fn member<D: Directory>(
    owner: &D,
    hash: u64,
    matches: impl Fn(map::raw::Occupied) -> bool,
) -> map::raw::Occupied {
    let mut index = hash as usize & owner.buckets().wrapping_sub(1);
    loop {
        let current = index;
        index = (index + 1) & owner.buckets().wrapping_sub(1);
        if !matches!(
            unsafe { owner.candidate(current, hash) },
            Candidate::Occupied
        ) {
            continue;
        }
        let bucket = unsafe { map::raw::Occupied::new(current) };
        if matches(bucket) {
            return bucket;
        }
    }
}

/// # Safety
/// The hole belongs to this nonempty directory, which retains a spare bucket.
pub(in crate::depot::map) unsafe fn close<D: Shift>(owner: &mut D, mut hole: map::raw::Vacant) {
    let mask = owner.buckets() - 1;
    loop {
        let mut next = (hole.index() + 1) & mask;
        loop {
            let Some(hash) = (unsafe { owner.hash(next) }) else {
                return;
            };
            let home = hash as usize & mask;
            if next.wrapping_sub(home) & mask > hole.index().wrapping_sub(home) & mask {
                let occupied = unsafe { map::raw::Occupied::new(next) };
                unsafe { owner.shift(&hole, &occupied, hash) };
                hole = unsafe { map::raw::Vacant::new(next) };
                break;
            }
            next = (next + 1) & mask;
        }
    }
}

type Fields<P> = alloc::raw::Tail<
    alloc::raw::Tail<alloc::raw::Layout<u8>, mem::MaybeUninit<u64>>,
    mem::MaybeUninit<P>,
>;

pub struct Layout<P> {
    bundle: alloc::raw::Bundle<Fields<P>>,
    buckets: usize,
    shape: Shape<P>,
}

#[repr(transparent)]
pub struct Shape<P> {
    plan: map::Plan,
    value: marker::PhantomData<fn() -> P>,
}

impl<P> Layout<P> {
    pub(crate) const fn bundle(&self) -> &alloc::raw::Bundle<Fields<P>> {
        &self.bundle
    }

    pub const fn buckets(&self) -> usize {
        self.buckets
    }

    pub const fn shape(&self) -> Shape<P> {
        self.shape
    }

    pub fn try_plan(plan: map::Plan) -> Result<Self, crate::alloc::Overflow> {
        let buckets = plan.buckets();
        let bundle = alloc::raw::Recipe::try_build(Self::recipe(buckets))?;
        Ok(Self {
            bundle,
            buckets,
            shape: Shape {
                plan,
                value: marker::PhantomData,
            },
        })
    }

    fn recipe(buckets: usize) -> impl alloc::raw::Recipe<Fields = Fields<P>> {
        use alloc::raw::Recipe as _;

        alloc::raw::Root::<u8>::new(buckets)
            .tail::<mem::MaybeUninit<u64>>(buckets)
            .tail::<mem::MaybeUninit<P>>(buckets)
    }
}

impl<P> Shape<P> {
    pub const fn capacity(self) -> capacity::Count {
        self.plan.capacity()
    }

    pub fn layout(self) -> Layout<P> {
        let buckets = self.plan.buckets();
        // SAFETY: this value-bound shape was admitted with the same recipe.
        let bundle = unsafe { alloc::raw::Recipe::build(Layout::<P>::recipe(buckets)) };
        Layout {
            bundle,
            buckets,
            shape: self,
        }
    }
}

impl<P> Clone for Shape<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for Shape<P> {}

impl<P> alloc::raw::Recipe for Layout<P> {
    type Fields = Fields<P>;

    fn try_build(self) -> Result<alloc::raw::Bundle<Self::Fields>, alloc::Overflow> {
        Ok(self.bundle)
    }

    unsafe fn build(self) -> alloc::raw::Bundle<Self::Fields> {
        self.bundle
    }
}

/// # Safety
/// The owner loan retains the same directory and occupied payloads. Projection must preserve their exact owner, bucket, and initialized lifetime.
/// If `SPARE` is true, every nonempty directory must retain a vacant bucket throughout the query.
pub unsafe trait Query: Sized {
    /// Carries the owner's nonempty-directory vacancy guarantee into probing.
    const SPARE: bool = false;

    type Payload;
    type Value;
    type Occupied;

    fn table(&self) -> &map::raw::Table<Self::Payload>;

    fn value(entry: map::Found<&Self>) -> &Self::Value;

    fn occupied(entry: map::Found<Self>) -> Self::Occupied;
}

/// Carries the original owner into its insertion capability.
/// # Safety
/// Insertion must retain the vacancy in the same owner and publish its exact hash and payload before making the bucket occupied.
pub unsafe trait Insertion: Query {
    type Vacant;

    fn vacant(entry: map::Vacancy<Self>, hash: u64) -> Option<Self::Vacant>;
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Occupied(num::NonZero<u32>);

#[repr(transparent)]
pub struct Vacant(u32);

impl Occupied {
    /// # Safety
    /// `index` is below the bucket count of the retained `Layout` directory (at most 2^31).
    /// That exact directory retains its initialized occupied bucket until use.
    pub const unsafe fn new(index: usize) -> Self {
        Self(unsafe { num::NonZero::<u32>::new_unchecked(index as u32 + 1) })
    }

    pub const fn index(&self) -> usize {
        self.0.get() as usize - 1
    }
}

impl Vacant {
    pub const fn index(&self) -> usize {
        self.0 as usize
    }

    /// `index` identifies a vacant bucket in a directory made by `Plan`.
    pub(crate) const unsafe fn new(index: usize) -> Self {
        Self(index as u32)
    }
}

const EMPTY: u8 = 1 << 7;

pub struct Table<P> {
    controls: ptr::NonNull<u8>,
    hashes: ptr::NonNull<mem::MaybeUninit<u64>>,
    payloads: ptr::NonNull<mem::MaybeUninit<P>>,
    buckets: usize,
}

/// Mutates one directory and its reverse indices under the same owner.
/// # Safety
/// Exclusive mutations must preserve the retained table allocation and every occupied payload. Shifts must update the exact directory and reverse indices.
pub unsafe trait Mutation: map::raw::Query {
    /// # Safety
    /// The caller preserves the exact directory and its live payloads.
    unsafe fn table_mut(&mut self) -> &mut Table<Self::Payload>;
    /// # Safety
    /// Both buckets belong to this owner; `next` is live with `hash`, and `hole` is vacant.
    unsafe fn shift(&mut self, hole: &map::raw::Vacant, next: &map::raw::Occupied, hash: u64);
}

pub struct Hole<Q> {
    owner: Q,
}

impl<P> Table<P> {
    pub(crate) fn try_new(layout: map::raw::Layout<P>) -> Result<Self, alloc::Exhausted> {
        let fields = layout.bundle().try_allocate()?;
        // SAFETY: allocation binds these exact planes and bucket count to this layout.
        Ok(unsafe { Self::from_fields(fields, layout.buckets()) })
    }

    /// # Safety
    /// These exact planes and bucket count were admitted by one `Layout`: zero or a power of two, at most 2^31.
    /// All planes stay live, disjoint and exclusively owned through access and destruction; controls start uninitialized.
    pub unsafe fn from_fields(
        fields: alloc::raw::Fields<'_, <map::raw::Layout<P> as alloc::raw::Recipe>::Fields>,
        buckets: usize,
    ) -> Self {
        let table = Self {
            controls: fields.prior().prior().root(),
            hashes: fields.prior().tail(),
            payloads: fields.tail(),
            buckets,
        };
        if table.buckets != 0 {
            unsafe { ptr::write_bytes(table.controls.as_ptr(), EMPTY, table.buckets) };
        }
        table
    }

    pub const fn base(&self) -> ptr::NonNull<u8> {
        self.controls
    }
}

impl<P> Table<P> {
    /// # Safety
    /// `index` must identify a bucket in this table.
    pub(crate) unsafe fn is_occupied(&self, index: usize) -> bool {
        unsafe { *self.controls.as_ptr().add(index) != EMPTY }
    }

    /// # Safety
    /// The bucket identifies an initialized payload in this exact live directory.
    pub unsafe fn payload(&self, bucket: &map::raw::Occupied) -> &P {
        let index = bucket.index();
        unsafe { (&*self.payloads.as_ptr().add(index)).assume_init_ref() }
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &P> {
        (0..self.buckets).filter_map(|index| {
            if !unsafe { self.is_occupied(index) } {
                return None;
            }
            Some(unsafe { self.payload(&map::raw::Occupied::new(index)) })
        })
    }
}

impl<P> Table<P> {
    /// # Safety
    /// The bucket is occupied and initialized in this live directory; mutation preserves its hash and ownership.
    pub unsafe fn payload_mut(&mut self, bucket: &map::raw::Occupied) -> &mut P {
        let index = bucket.index();
        unsafe { (&mut *self.payloads.as_ptr().add(index)).assume_init_mut() }
    }

    pub fn clear(&mut self) {
        if !mem::needs_drop::<P>() {
            // SAFETY: this exclusive owner retains the complete control plane;
            // P has no destruction obligation before its membership is cleared.
            unsafe { self.controls.as_ptr().write_bytes(EMPTY, self.buckets) };
            return;
        }
        for index in 0..self.buckets {
            if !unsafe { self.is_occupied(index) } {
                continue;
            }
            unsafe {
                self.controls.as_ptr().add(index).write(EMPTY);
                (&mut *self.payloads.as_ptr().add(index)).assume_init_drop();
            }
        }
    }

    /// Publishes one bucket and returns its initialized value under the original loan.
    /// # Safety
    /// The bucket is vacant in this directory and was probed for this exact hash.
    pub unsafe fn insert(&mut self, bucket: map::raw::Vacant, hash: u64, payload: P) -> &mut P {
        let index = bucket.index();
        let hashes = self.hashes;
        let payloads = self.payloads;
        let controls = self.controls;
        unsafe {
            hashes
                .as_ptr()
                .add(index)
                .write(mem::MaybeUninit::new(hash));
            payloads
                .as_ptr()
                .add(index)
                .write(mem::MaybeUninit::new(payload));
            controls.as_ptr().add(index).write((hash >> 57) as u8);
            (&mut *payloads.as_ptr().add(index)).assume_init_mut()
        }
    }

    /// # Safety
    /// Both buckets belong to this directory; `next` is initialized with `hash`, and `hole` is vacant.
    pub unsafe fn shift(
        &mut self,
        hole: &map::raw::Vacant,
        next: &map::raw::Occupied,
        hash: u64,
    ) -> (map::raw::Occupied, &mut P) {
        let hole = hole.index();
        let next = next.index();
        unsafe {
            let payload = self.payloads.as_ptr().add(next).read();
            self.hashes
                .as_ptr()
                .add(hole)
                .write(mem::MaybeUninit::new(hash));
            self.payloads.as_ptr().add(hole).write(payload);
            let control = *self.controls.as_ptr().add(next);
            self.controls.as_ptr().add(hole).write(control);
            self.controls.as_ptr().add(next).write(EMPTY);
            (
                map::raw::Occupied::new(hole),
                (&mut *self.payloads.as_ptr().add(hole)).assume_init_mut(),
            )
        }
    }
}

impl<Q: Mutation> Hole<Q> {
    /// # Safety
    /// The bucket is occupied in this exact owner; a spare bucket terminates backshift.
    pub unsafe fn remove(mut owner: Q, bucket: map::raw::Occupied) -> Q::Payload {
        let value = unsafe {
            owner
                .table_mut()
                .payloads
                .as_ptr()
                .add(bucket.index())
                .read()
                .assume_init()
        };
        Self::close(owner, bucket);
        value
    }

    pub(crate) unsafe fn retire(owner: Q, bucket: map::raw::Occupied)
    where
        Q::Payload: Copy,
    {
        Self::close(owner, bucket);
    }

    fn close(mut owner: Q, bucket: map::raw::Occupied) {
        let index = bucket.index();
        unsafe { owner.table_mut().controls.as_ptr().add(index).write(EMPTY) };
        let mut hole = Self { owner };
        // SAFETY: the original owner retains this exact directory after vacating its live bucket.
        unsafe { close(&mut hole, map::raw::Vacant::new(index)) };
    }
}

// SAFETY: Hole retains the original mutation owner and its unchanged directory.
unsafe impl<Q: Mutation> Directory for Hole<Q> {
    fn buckets(&self) -> usize {
        self.owner.table().buckets
    }

    unsafe fn hash(&self, index: usize) -> Option<u64> {
        unsafe { self.owner.table().hash(index) }
    }
}

// SAFETY: Mutation updates the exact retained directory and its reverse indices.
unsafe impl<Q: Mutation> Shift for Hole<Q> {
    unsafe fn shift(&mut self, hole: &Vacant, next: &Occupied, hash: u64) {
        unsafe { self.owner.shift(hole, next, hash) };
    }
}

const _: () = {
    assert!(mem::size_of::<Table<u64>>() == 4 * mem::size_of::<usize>());
    assert!(mem::size_of::<Hole<&mut map::flat::Table<u64>>>() == mem::size_of::<usize>());
};

/// Retains the exact query owner with the bucket proven by its directory scan.
/// Construction is restricted to the shared map kernel; consuming it preserves that pairing.
pub(crate) struct Bucket<Q, B> {
    owner: Q,
    bucket: B,
}

impl<Q, B> Bucket<Q, B> {
    pub(crate) fn into_parts(self) -> (Q, B) {
        (self.owner, self.bucket)
    }
}
