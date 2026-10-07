use crate::mem::credit::fair;

/// A borrowed pool view bound to one independently owned lane state.
pub struct Lane<'p, 's, const N: usize = 1, Tag = (), W: crate::mem::raw::Word = usize> {
    pub(super) pool: &'p crate::mem::raw::Pool<N, Tag, W>,
    pub(super) state: &'s crate::mem::raw::State<N, Tag, W>,
}

impl<const N: usize, Tag, W: crate::mem::raw::Word> Clone for Lane<'_, '_, N, Tag, W> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<const N: usize, Tag, W: crate::mem::raw::Word> Copy for Lane<'_, '_, N, Tag, W> {}

// SAFETY: Lane stores the original pool and state borrows without rebinding.
unsafe impl<const N: usize, Tag, W: crate::mem::raw::Word> crate::mem::raw::Source<N>
    for Lane<'_, '_, N, Tag, W>
{
    type Tag = Tag;
    type Word = W;

    unsafe fn accounts(&self) -> crate::mem::raw::Accounts<'_, N, Tag, W> {
        (&self.pool.shared, self.state)
    }
}

impl<'p, 's, const N: usize, Tag, W: crate::mem::raw::Word> Lane<'p, 's, N, Tag, W> {
    /// Computes incremental shared demand for the current accounts without admission.
    pub fn demand(self, amount: [W; N]) -> [W; N] {
        let balance = crate::mem::raw::Balance::new(self.state.held.get(), self.pool.shared.get());
        balance.demand(self.state.reserve, amount)
    }

    pub fn fits(self, amount: [W; N]) -> bool {
        (0..N).all(|axis| {
            amount[axis] <= self.pool.limit[axis].wrapping_add(self.state.reserve[axis])
        })
    }

    /// # Safety
    /// Every sum fits `W` and each shared demand fits current surplus; both bounds
    /// remain valid through `Amount::get`, including reentrant work before this lane's accounts are read.
    pub unsafe fn acquire_raw<A: crate::mem::raw::Amount<N, W>>(
        self,
        amount: A,
    ) -> crate::mem::raw::Charge<Self, N, A> {
        let raw = amount.get();
        let balance = crate::mem::raw::Balance::new(self.state.held.get(), self.pool.shared.get());
        // SAFETY: the caller proves every sum and shared demand for this lane.
        let (next, shared) = unsafe { balance.acquire_raw(self.state.reserve, raw) }.into_parts();
        self.state.held.set(next);
        self.pool.shared.set(shared);
        crate::mem::raw::Charge::new(self, amount)
    }

    pub fn try_acquire<A: crate::mem::raw::Amount<N, W>>(
        self,
        amount: A,
    ) -> Result<crate::mem::raw::Charge<Self, N, A>, crate::mem::Rejected<A, fair::Cause<W, N>>>
    {
        crate::mem::raw::Charge::try_new(self, amount)
    }
}
