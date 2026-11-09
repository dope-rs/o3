use std::{cell, marker, mem, num, ptr, slice};

use crate::{
    alloc,
    alloc::array,
    buffer::{
        pool,
        pool::{owners, state},
    },
};

/// A pool that allocates bounded geometric groups on demand.
/// Groups stay at stable addresses, preserving both owned and borrowed slots
/// across later growth.
pub struct Growing {
    groups: array::Block<cell::Cell<mem::MaybeUninit<pool::Pool>>, usize>,
    plan: pool::Layout,
    allocated: cell::Cell<u32>,
    initialized: cell::Cell<u8>,
    hint: cell::Cell<u8>,
}

impl Growing {
    /// Creates the first slot group eagerly and reserves every group pointer.
    pub fn try_new(plan: pool::Layout) -> Result<Self, alloc::Exhausted> {
        Self::try_first(plan, plan.next(0))
    }

    /// Allocates exactly `warm` initial slots without reducing the growth bound.
    /// Rejects a warm group larger than the plan before allocating anything.
    pub fn try_warm(
        plan: pool::Layout,
        warm: num::NonZero<u32>,
    ) -> Result<Self, pool::GrowingError> {
        let first = plan.try_prefix(warm)?;
        Self::try_first(plan, Some(first)).map_err(pool::GrowingError::Exhausted)
    }

    fn try_first(
        plan: pool::Layout,
        first: Option<pool::NonEmpty>,
    ) -> Result<Self, alloc::Exhausted> {
        let groups = array::Plan::from(Self::count(plan, first))
            .try_allocate(())?
            .repeat_with(|| cell::Cell::new(mem::MaybeUninit::uninit()));
        let pool = Self {
            groups,
            plan,
            allocated: cell::Cell::new(0),
            initialized: cell::Cell::new(0),
            hint: cell::Cell::new(0),
        };
        if let Some(first) = first {
            pool.insert(pool::Pool::try_from(first.into_layout())?);
        }
        Ok(pool)
    }

    /// Returns unclaimed slots, including groups not yet allocated.
    /// Allocating those groups may still exhaust memory.
    pub fn available(&self) -> usize {
        let vacant = self.plan.slots() - self.allocated.get();
        vacant as usize
            + self
                .initialized()
                .iter()
                .map(pool::Pool::available)
                .sum::<usize>()
    }

    /// Returns whether every configured slot is already claimed.
    pub fn is_full(&self) -> bool {
        self.plan.slots() == self.allocated.get()
            && self.initialized().iter().all(|pool| pool.available() == 0)
    }

    /// Acquires an owned lease, growing only when existing groups are full.
    pub fn try_acquire(&mut self) -> Result<Option<pool::Span>, alloc::Exhausted> {
        Ok(self
            .try_take()?
            .map(|active| pool::Span::new(active, pool::Owned)))
    }

    /// Acquires a slot tied to this pool borrow, retaining it through growth.
    pub fn try_borrow(
        &self,
    ) -> Result<Option<pool::Span<owners::Plain, pool::Borrowed<'_>>>, alloc::Exhausted> {
        Ok(self
            .try_take()?
            .map(|active| pool::Span::new(active, pool::Borrowed(marker::PhantomData))))
    }

    fn try_take(
        &self,
    ) -> Result<Option<owners::Active<owners::Plain, state::Uninit>>, alloc::Exhausted> {
        loop {
            let groups = self.initialized();
            let hint = usize::from(self.hint.get());
            if let Some(group) = groups.get(hint)
                && let Some(active) = owners::Active::acquire(group.core)
            {
                return Ok(Some(active));
            }
            for (index, group) in groups.iter().enumerate() {
                if index != hint
                    && let Some(active) = owners::Active::acquire(group.core)
                {
                    self.hint.set(index as u8);
                    return Ok(Some(active));
                }
            }
            let before = self.allocated.get();
            let Some(layout) = self.plan.next(before as usize) else {
                return Ok(None);
            };
            let seed = owners::Block::<owners::Plain, state::Uninit>::try_seed(layout)?;
            let active = seed.take();
            let group = pool::Pool { core: active.slots };
            if self.allocated.get() != before {
                continue;
            }
            self.insert(group);
            return Ok(Some(active));
        }
    }

    fn count(plan: pool::Layout, first: Option<pool::NonEmpty>) -> u8 {
        first.map_or(0, |first| {
            let warm = first.into_layout().slots();
            let shift = (plan.slots().bit_width() - warm.bit_width()) & 31;
            (1 + shift + u32::from((warm << shift) < plan.slots())) as u8
        })
    }

    /// Borrows the monotonically initialized prefix at its permanent address.
    fn initialized(&self) -> &[pool::Pool] {
        // SAFETY: insert publishes each previously vacant cell exactly once.
        // Growth writes only after this prefix; no pool moves before Drop.
        unsafe {
            slice::from_raw_parts(
                self.groups.as_ptr().cast(),
                usize::from(self.initialized.get()),
            )
        }
    }

    fn insert(&self, pool: pool::Pool) {
        let slots = pool.slots() as u32;
        let index = self.initialized.get();
        // SAFETY: the nonempty first group and subsequent Layout::next steps
        // at least double the allocated count until its last bounded group.
        // count reserves the exact bound of at most 33 groups; try_take also checks
        // its admission survived allocator reentry. Each insertion owns the
        // next vacant cell without relocating readers.
        unsafe {
            self.groups
                .get_unchecked(usize::from(index))
                .set(mem::MaybeUninit::new(pool));
        }
        self.allocated.set(self.allocated.get() + slots);
        self.initialized.set(index + 1);
        self.hint.set(index);
    }
}

impl Drop for Growing {
    fn drop(&mut self) {
        // SAFETY: exclusive destruction owns exactly this initialized prefix;
        // its backing then frees the remaining MaybeUninit cells.
        unsafe {
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(
                self.groups.as_ptr().cast_mut().cast::<pool::Pool>(),
                usize::from(self.initialized.get()),
            ));
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Growing>() == 4 * mem::size_of::<usize>());
    assert!(mem::size_of::<Result<Growing, alloc::Exhausted>>() == mem::size_of::<Growing>());
};
