use std::convert::Infallible;

use crate::depot::map;

pub struct Probe<Q> {
    owner: Q,
    state: State,
    hash: u64,
}

pub(in crate::depot::map) enum State {
    Occupied(map::raw::Occupied),
    Vacant(map::raw::Vacant),
    Full,
}

/// Physical access retained by one scalar directory owner.
/// # Safety
/// The extent is zero or a power of two at most 2^31; occupied buckets retain initialized same-owner records and stable identities under shared loans. Hashes select placement, not initialization.
pub(in crate::depot::map) unsafe trait Directory {
    fn buckets(&self) -> usize;

    /// # Safety
    /// The index is below this directory's bucket count.
    unsafe fn hash(&self, index: usize) -> Option<u64>;

    /// Vacant names an empty bucket; Occupied and Skip name initialized same-owner records.
    /// # Safety
    /// The index is below the retained bucket count; Occupied may include hash false positives.
    unsafe fn candidate(&self, index: usize, hash: u64) -> Candidate {
        match unsafe { self.hash(index) } {
            None => Candidate::Vacant,
            Some(actual) => {
                if actual == hash {
                    Candidate::Occupied
                } else {
                    Candidate::Skip
                }
            }
        }
    }
}

pub(in crate::depot::map) enum Candidate {
    Occupied,
    Vacant,
    Skip,
}

/// Exclusive bucket mutation under the original directory owner.
///
/// # Safety
/// Shifting preserves every live record and updates any reverse directory.
pub(in crate::depot::map) unsafe trait Shift: Directory {
    /// # Safety
    /// Both buckets belong to this owner; next is occupied and hole is vacant.
    unsafe fn shift(&mut self, hole: &map::raw::Vacant, next: &map::raw::Occupied, hash: u64);
}

// SAFETY: Table retains the exact initialized control, hash and payload planes.
unsafe impl<P> Directory for map::raw::Table<P> {
    fn buckets(&self) -> usize {
        self.buckets
    }

    unsafe fn hash(&self, index: usize) -> Option<u64> {
        if unsafe { *self.controls.as_ptr().add(index) } == 1 << 7 {
            None
        } else {
            Some(unsafe { (*self.hashes.as_ptr().add(index)).assume_init() })
        }
    }

    unsafe fn candidate(&self, index: usize, hash: u64) -> Candidate {
        let control = unsafe { *self.controls.as_ptr().add(index) };
        if control == 1 << 7 {
            Candidate::Vacant
        } else if control == (hash >> 57) as u8
            && unsafe { (*self.hashes.as_ptr().add(index)).assume_init() } == hash
        {
            Candidate::Occupied
        } else {
            Candidate::Skip
        }
    }
}

/// A scalar scan retains the exact directory whose occupied buckets it reads.
pub(super) struct Scan<'a, D, const SPARE: bool> {
    table: &'a D,
    hash: u64,
    index: usize,
    left: usize,
}

impl<'a, D: Directory, const SPARE: bool> Scan<'a, D, SPARE> {
    pub(super) fn new(table: &'a D, hash: u64) -> Self {
        Self {
            table,
            hash,
            index: hash as usize & table.buckets().wrapping_sub(1),
            left: table.buckets(),
        }
    }
}

impl<D: Directory> Scan<'_, D, false> {
    pub(super) fn next(&mut self) -> State {
        while self.left != 0 {
            self.left -= 1;
            match self.step() {
                Some(map::Entry::Occupied(bucket)) => return State::Occupied(bucket),
                Some(map::Entry::Vacant(bucket)) => return State::Vacant(bucket),
                None => {}
            }
        }
        State::Full
    }
}

impl<D: Directory> Scan<'_, D, true> {
    /// The retained owner guarantees termination at a vacant bucket.
    pub(super) fn next(&mut self) -> map::Entry<map::raw::Occupied, map::raw::Vacant> {
        loop {
            if let Some(entry) = self.step() {
                return entry;
            }
        }
    }
}

impl<D: Directory, const SPARE: bool> Scan<'_, D, SPARE> {
    fn step(&mut self) -> Option<map::Entry<map::raw::Occupied, map::raw::Vacant>> {
        let index = self.index;
        // SAFETY: a nonempty scan starts and advances within its retained extent.
        let kind = unsafe { self.table.candidate(index, self.hash) };
        if matches!(kind, Candidate::Vacant) {
            return Some(map::Entry::Vacant(unsafe { map::raw::Vacant::new(index) }));
        }
        self.index = (index + 1) & self.table.buckets().wrapping_sub(1);
        matches!(kind, Candidate::Occupied)
            .then(|| map::Entry::Occupied(unsafe { map::raw::Occupied::new(index) }))
    }
}

impl<Q: map::raw::Query> Probe<Q> {
    /// # Safety
    /// The retained owner has a nonempty directory with a vacant bucket until consumption.
    pub(crate) unsafe fn vacant(owner: Q, hash: u64) -> map::Vacancy<Q, u64> {
        let mut scan = Scan::<_, true>::new(owner.table(), hash);
        loop {
            if let map::Entry::Vacant(bucket) = scan.next() {
                return map::Vacancy::new(map::raw::Bucket { owner, bucket }).with_phase(hash);
            }
        }
    }

    /// # Safety
    /// The retained owner has a nonempty directory with a vacant bucket until consumption.
    pub(crate) unsafe fn spare(
        owner: Q,
        hash: u64,
        matches: impl Fn(&Q::Value) -> bool,
    ) -> map::Entry<map::Found<Q>, map::Vacancy<Q, u64>> {
        let mut scan = Scan::<_, true>::new(owner.table(), hash);
        loop {
            let bucket = match scan.next() {
                map::Entry::Occupied(bucket) => bucket,
                map::Entry::Vacant(bucket) => {
                    return map::Entry::Vacant(
                        map::Vacancy::new(map::raw::Bucket { owner, bucket }).with_phase(hash),
                    );
                }
            };
            if matches(Q::value(map::Found::new(map::raw::Bucket {
                owner: &owner,
                bucket,
            }))) {
                return map::Entry::Occupied(map::Found::new(map::raw::Bucket { owner, bucket }));
            }
        }
    }

    /// # Safety
    /// An occupied entry in this exact owner has `hash` and satisfies `matches` until returned.
    pub(crate) unsafe fn member(
        owner: Q,
        hash: u64,
        matches: impl Fn(&Q::Value) -> bool,
    ) -> map::Found<Q> {
        let bucket = unsafe {
            map::raw::member(owner.table(), hash, |bucket| {
                matches(Q::value(map::Found::new(map::raw::Bucket {
                    owner: &owner,
                    bucket,
                })))
            })
        };
        map::Found::new(map::raw::Bucket { owner, bucket })
    }

    pub(crate) fn new(owner: Q, hash: u64, matches: impl Fn(&Q::Value) -> bool) -> Self {
        let state = Self::scan(&owner, hash, &matches);
        Self { owner, state, hash }
    }

    fn scan(owner: &Q, hash: u64, matches: &impl Fn(&Q::Value) -> bool) -> State {
        match Self::try_scan(owner, hash, &|value| Ok::<_, Infallible>(matches(value))) {
            Ok(state) => state,
            Err(error) => match error {},
        }
    }

    pub(crate) fn try_new<E>(
        owner: Q,
        hash: u64,
        matches: impl Fn(&Q::Value) -> Result<bool, E>,
    ) -> Result<Self, E> {
        let state = Self::try_scan(&owner, hash, &matches)?;
        Ok(Self { owner, state, hash })
    }

    fn try_scan<E>(
        owner: &Q,
        hash: u64,
        matches: &impl Fn(&Q::Value) -> Result<bool, E>,
    ) -> Result<State, E> {
        let matches = |bucket| {
            matches(Q::value(map::Found::new(map::raw::Bucket {
                owner,
                bucket,
            })))
        };
        if Q::SPARE {
            map::raw::scan::<_, _, true>(owner.table(), hash, matches)
        } else {
            map::raw::scan::<_, _, false>(owner.table(), hash, matches)
        }
    }

    pub fn find(owner: Q, hash: u64, matches: impl Fn(&Q::Value) -> bool) -> Option<Q::Occupied> {
        let State::Occupied(bucket) = Self::scan(&owner, hash, &matches) else {
            return None;
        };
        Some(Q::occupied(map::Found::new(map::raw::Bucket {
            owner,
            bucket,
        })))
    }

    pub(crate) fn try_find<E>(
        owner: Q,
        hash: u64,
        matches: impl Fn(&Q::Value) -> Result<bool, E>,
    ) -> Result<Option<Q::Occupied>, E> {
        let State::Occupied(bucket) = Self::try_scan(&owner, hash, &matches)? else {
            return Ok(None);
        };
        Ok(Some(Q::occupied(map::Found::new(map::raw::Bucket {
            owner,
            bucket,
        }))))
    }
}

impl<Q: map::raw::Insertion> Probe<Q> {
    pub(crate) fn entry(self) -> map::Entry<Q::Occupied, Option<Q::Vacant>> {
        match self.state {
            State::Occupied(bucket) => {
                map::Entry::Occupied(Q::occupied(map::Found::new(map::raw::Bucket {
                    owner: self.owner,
                    bucket,
                })))
            }
            State::Vacant(bucket) => map::Entry::Vacant(Q::vacant(
                map::Vacancy::new(map::raw::Bucket {
                    owner: self.owner,
                    bucket,
                }),
                self.hash,
            )),
            State::Full => map::Entry::Vacant(None),
        }
    }

    pub fn reserve(
        owner: Q,
        hash: u64,
        matches: impl Fn(&Q::Value) -> bool,
    ) -> map::Entry<(), Option<Q::Vacant>> {
        match Self::scan(&owner, hash, &matches) {
            State::Occupied(_) => map::Entry::Occupied(()),
            State::Vacant(bucket) => map::Entry::Vacant(Q::vacant(
                map::Vacancy::new(map::raw::Bucket { owner, bucket }),
                hash,
            )),
            State::Full => map::Entry::Vacant(None),
        }
    }
}

const _: () = {
    assert!(size_of::<map::raw::Occupied>() == size_of::<u32>());
    assert!(size_of::<Option<map::raw::Occupied>>() == size_of::<map::raw::Occupied>());
    assert!(size_of::<map::raw::Vacant>() == size_of::<u32>());
    assert!(size_of::<State>() == size_of::<u64>());
    assert!(
        size_of::<Probe<&map::flat::Table<u64>>>()
            == size_of::<(&map::flat::Table<u64>, u64, u64)>()
    );
};

/// Owns a nonempty directory with at least one vacant bucket until consumed.
/// # Safety
/// The retained owner must keep at least one vacant bucket throughout probing and transfer that same vacancy into the returned insertion capability.
pub(crate) unsafe trait Spare: map::raw::Query {
    fn try_reserve<Q: Eq + ?Sized>(
        self,
        hash: u64,
        key: &Q,
    ) -> Result<map::Vacancy<Self, u64>, Self::Occupied>
    where
        Self::Value: AsRef<Q>,
    {
        // SAFETY: this consumed admission retains the same spare directory through probing.
        match unsafe { Probe::spare(self, hash, |value| value.as_ref() == key) } {
            map::Entry::Occupied(entry) => Err(Self::occupied(entry)),
            map::Entry::Vacant(entry) => Ok(entry),
        }
    }
}
