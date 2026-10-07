use std::{array, cell, fmt::Debug, mem::MaybeUninit, num::NonZero, ops, ptr};

pub(crate) use state::{Account, Live};

use crate::{
    base::marker::ThreadBound,
    mem::{rc, slot},
};
mod balance;
pub use balance::Balance;
mod bound;
pub use bound::Bound;
mod charge;
pub use charge::Charge;
mod grant;
pub use grant::Grant;
mod lane;
pub use lane::Lane;
mod pool;
pub use pool::Pool;
mod state;
pub use state::State;
mod owner;
pub use owner::Owner;

/// An invariant accounting domain confined to its constructing thread.
type Domain<Tag, W = ()> = std::marker::PhantomData<(fn(Tag) -> Tag, W, ThreadBound)>;

/// Reborrows uninitialized storage for the same number of fitting values.
/// This view does not reinterpret initialized values or own their destruction.
pub(crate) fn cast_uninit<T, U, const N: usize>(
    source: &mut [MaybeUninit<T>; N],
) -> &mut [MaybeUninit<U>; N] {
    const {
        assert!(size_of::<U>() <= size_of::<T>(), "size");
        assert!(align_of::<T>().is_multiple_of(align_of::<U>()), "align");
    };
    // SAFETY: the target array fits the source extent at compatible alignment.
    // Its elements use U's stride; MaybeUninit permits the untouched contents.
    // The exclusive view retains the original borrow and allocation owner.
    unsafe { &mut *ptr::from_mut(source).cast::<[MaybeUninit<U>; N]>() }
}

/// Exclusive access to an initialized value whose storage retains destruction.
/// Ending this loan leaves the value and its destruction owned by the storage.
#[derive(Debug)]
#[repr(transparent)]
pub struct Occupied<'a, T> {
    source: &'a mut Option<T>,
    bound: ThreadBound,
}

/// Exclusive admission to initialize one empty owned cell.
#[derive(Debug)]
#[must_use]
#[repr(transparent)]
pub struct Vacant<'a, T> {
    source: &'a mut Option<T>,
    bound: ThreadBound,
}

/// # Safety
/// This exact cell is initialized and its lifecycle is frozen for the loan.
pub(crate) unsafe fn project<T, M>(source: &slot::Storage<T, M>) -> &T {
    unsafe { initialized(&source.value) }
}

/// # Safety
/// `source` is `Some` and remains initialized for the returned loan.
pub(crate) unsafe fn initialized<T>(source: &Option<T>) -> &T {
    unsafe { source.as_slice().get_unchecked(0) }
}

/// # Safety
/// This exact cell is initialized and its lifecycle is frozen for the loan.
pub(crate) unsafe fn project_mut<T, M>(source: &mut slot::Storage<T, M>) -> &mut T {
    unsafe { &mut *source.value.as_mut_slice().as_mut_ptr() }
}

/// # Safety
/// This exact cell is initialized. End every authority depending on that
/// initialization before transferring this exclusive take capability.
pub(crate) unsafe fn occupy<T, M>(storage: &mut slot::Storage<T, M>) -> Occupied<'_, T> {
    Occupied {
        source: &mut storage.value,
        bound: ThreadBound::NEW,
    }
}

/// # Safety
/// This exact cell is vacant and exclusively borrowed until initialization.
pub(crate) unsafe fn vacancy<T, M>(storage: &mut slot::Storage<T, M>) -> Vacant<'_, T> {
    Vacant {
        source: &mut storage.value,
        bound: ThreadBound::NEW,
    }
}

impl<'a, T> Occupied<'a, T> {
    pub fn clear(self) {
        *self.source = None;
    }

    pub fn get(&self) -> &T {
        unsafe { initialized(self.source) }
    }

    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: this short loan projects the retained Some's sole value.
        unsafe { &mut *self.source.as_mut_slice().as_mut_ptr() }
    }

    /// Transfers the retained initialized loan without ending storage ownership.
    pub fn into_mut(self) -> &'a mut T {
        // SAFETY: consuming this capability transfers its original exclusive loan.
        // Storage keeps the same initialized value and destruction obligation.
        unsafe { &mut *self.source.as_mut_slice().as_mut_ptr() }
    }

    pub fn take(self) -> (T, Vacant<'a, T>) {
        // SAFETY: this consumed capability moves the selected value once, then
        // clears its owner's stale representation without dropping it. No value
        // pointer is accessed after clearing and no user code runs between them.
        let value = unsafe {
            let value = self.source.as_mut_slice().as_mut_ptr().read();
            ptr::write(self.source, None);
            value
        };
        (
            value,
            Vacant {
                source: self.source,
                bound: ThreadBound::NEW,
            },
        )
    }
}

impl<'a, T> Vacant<'a, T> {
    pub fn insert(self, value: T) -> Occupied<'a, T> {
        // SAFETY: the exclusive vacancy retains None; initializing it drops no value.
        unsafe { ptr::write(self.source, Some(value)) };
        Occupied {
            source: self.source,
            bound: ThreadBound::NEW,
        }
    }
}

/// Exact partition preserves the issuing source and every credited component.
type Partition<S, const N: usize, A, P> = (
    Grant<S, N, <P as Exact<N, <S as Source<N>>::Word, A>>::Left>,
    Grant<S, N, <P as Exact<N, <S as Source<N>>::Word, A>>::Right>,
);

/// Embedded thread-local count; only its nominal owner can transition it.
#[derive(Debug)]
#[repr(transparent)]
pub struct Count<W: Counter = u32> {
    value: cell::Cell<W>,
    thread: ThreadBound,
}

impl<W: Counter> Count<W> {
    pub const fn empty() -> Self {
        Self {
            value: cell::Cell::new(W::ZERO),
            thread: ThreadBound::NEW,
        }
    }

    pub const fn one() -> Self {
        Self {
            value: cell::Cell::new(W::ONE),
            thread: ThreadBound::NEW,
        }
    }
}

const _: () = assert!(size_of::<Count>() == size_of::<u32>());

/// One nominal owner domain projected onto its embedded local count.
/// # Safety
/// The returned count must remain the exact count for every edge operation.
pub unsafe trait Edge<W: Counter = u32> {
    fn count(&self) -> &crate::mem::raw::Count<W>;

    #[inline]
    fn is_empty(&self) -> bool {
        self.count().value.get() == W::ZERO
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.count().value.get() == W::ONE
    }

    /// Starts the first live owner with one store.
    /// # Safety
    /// The caller exclusively owns this empty domain until publication.
    #[inline]
    unsafe fn activate(&self) {
        self.count().value.set(W::ONE);
    }

    /// Starts exactly two live owners in an exclusively held empty domain.
    /// # Safety
    /// The caller publishes exactly two edges and consumes the exclusive owner.
    unsafe fn start_pair(&self) {
        self.count().value.set(W::TWO);
    }

    /// Transfers the sole counted edge into exclusive uncounted ownership.
    /// # Safety
    /// The caller consumes the unique live edge before exposing its replacement.
    unsafe fn retire(&self) {
        self.count().value.set(W::ZERO);
    }

    /// Adds one owner or leaves the exhausted domain unchanged.
    /// # Safety
    /// The caller already owns one live edge in this exact domain.
    #[inline]
    unsafe fn try_retain(&self) -> Result<(), rc::Error> {
        let count = self.count();
        // SAFETY: this exact domain is retained by the caller's live edge.
        let current = unsafe { Live::new(count.value.get()) };
        count.value.set(current.try_retain()?.get());
        Ok(())
    }

    /// Consumes exactly one live owner edge and stores the resulting count.
    /// # Safety
    /// The caller owns one edge and retires `Last` before reuse.
    #[inline]
    unsafe fn release(&self) -> rc::Release {
        let count = self.count();
        // SAFETY: the caller transfers one live edge from this exact domain.
        let (next, release) = match unsafe { Live::new(count.value.get()) }.release() {
            None => (W::ZERO, rc::Release::Last),
            Some(next) => (next.get(), rc::Release::Shared),
        };
        count.value.set(next);
        release
    }
}

/// # Safety
/// Safe target access preserves its exact count and allocation until all edges end.
/// Finalization consumes ownership once; empty domains cannot be reused during it.
pub unsafe trait Target<W: Counter = u32>: crate::mem::raw::Edge<W> + Sized {
    /// # Safety
    /// `pointer` owns the exact initialized allocation after its last counted
    /// edge ended. Its count is zero and no outstanding borrow can access it.
    unsafe fn destroy(pointer: ptr::NonNull<Self>);
}

/// Compact representation of one exact fair-credit amount.
/// # Safety
/// `get` must always return this value's exact immutable component vector.
pub unsafe trait Amount<const N: usize, W: crate::mem::raw::Word>: Copy + Eq {
    fn get(self) -> [W; N];
}

// SAFETY: the array is its own immutable component vector.
unsafe impl<const N: usize, W: crate::mem::raw::Word> Amount<N, W> for [W; N] {
    fn get(self) -> [W; N] {
        self
    }
}

/// Accounts borrowed from one exact source.
pub type Accounts<'a, const N: usize, Tag, W = usize> =
    (&'a cell::Cell<[W; N]>, &'a State<N, Tag, W>);

/// # Safety
/// Copies, projections and acquisitions retain the same stable live binding; copies create no credit.
/// Every reserve plus the initial shared surplus must fit `Self::Word`.
pub unsafe trait Source<const N: usize>: Copy {
    type Tag;
    type Word: Word;
    /// # Safety
    /// Read only, or conserve every outstanding acquisition when mutating these accounts.
    /// Never expose the shared cell through an API permitting unproved safe mutation.
    unsafe fn accounts(&self) -> Accounts<'_, N, Self::Tag, Self::Word>;

    fn held(&self) -> [Self::Word; N] {
        // SAFETY: this projection only reads the retained account.
        unsafe { self.accounts() }.1.held.get()
    }

    fn reserve(&self) -> [Self::Word; N] {
        // SAFETY: this projection only reads the retained account.
        unsafe { self.accounts() }.1.reserve
    }

    fn can_acquire(&self, amount: [Self::Word; N]) -> bool {
        // SAFETY: this projection only reads the retained accounts.
        let (shared, state) = unsafe { self.accounts() };
        Balance::new(state.held.get(), shared.get())
            .try_acquire(state.reserve, amount)
            .is_ok()
    }
}

// SAFETY: wrapping addition is exact once the admitted component sums fit W.
unsafe impl<const N: usize, W: Word> Sum<N, W> for [W; N] {
    unsafe fn add_raw(self, other: Self) -> Self {
        array::from_fn(|axis| self[axis].wrapping_add(other[axis]))
    }
}

/// Caller-proved sum for compact fair credit.
/// # Safety
/// `add_raw` returns the exact componentwise sum when every component fits.
pub unsafe trait Sum<const N: usize, W: Word>: Amount<N, W> {
    /// Adds two values after the caller proves that every component fits.
    /// # Safety
    /// The componentwise sum must be representable by `W`.
    unsafe fn add_raw(self, other: Self) -> Self;
}

/// # Safety
/// Once bound to its exact source, the marker must preserve every component.
pub unsafe trait ExactRebind<const N: usize, W: Word, A: Amount<N, W>>: Sized {
    type Output: Amount<N, W>;

    fn rebind(self, source: A) -> Self::Output;
}

/// # Safety
/// Preserve every component for every A, without caller-side preconditions.
pub unsafe trait Rebind<const N: usize, W: Word, A: Amount<N, W>>:
    ExactRebind<N, W, A>
{
}

/// # Safety
/// Once bound, return an exact partition of the source for every Input.
/// Input may change representation-only data, not the admitted partition.
pub unsafe trait Exact<const N: usize, W: Word, A: Amount<N, W>>: Sized {
    type Input;
    type Left: Amount<N, W>;
    type Right: Amount<N, W>;

    fn parts(self, source: A, input: Self::Input) -> (Self::Left, Self::Right);
}

/// # Safety
/// Conversions and representable arithmetic are exact; `wrap` retains low bits.
/// `checked_add` rejects overflow; `sub_or_zero` clamps negative differences to zero.
pub unsafe trait Word:
    Copy + Eq + Ord + ops::Add<Output = Self> + ops::Sub<Output = Self> + owner::Sealed
{
    const ZERO: Self;

    fn to_usize(self) -> usize;

    /// Converts the low bits of `value` into this word's representation.
    fn wrap(value: usize) -> Self;

    fn checked_add(self, other: Self) -> Option<Self>;

    fn sub_or_zero(self, other: Self) -> Self;

    fn wrapping_add(self, other: Self) -> Self {
        Self::wrap(self.to_usize().wrapping_add(other.to_usize()))
    }

    fn wrapping_sub(self, other: Self) -> Self {
        Self::wrap(self.to_usize().wrapping_sub(other.to_usize()))
    }
}

// SAFETY: u32 is sealed and 32 bits wide, so `to_usize` widens exactly, `wrap`
// keeps the low bits, `checked_add` is exact or rejects, and `sub_or_zero`
// saturates at zero.
unsafe impl Word for u32 {
    const ZERO: Self = 0;

    fn to_usize(self) -> usize {
        self as usize
    }

    fn checked_add(self, other: Self) -> Option<Self> {
        self.checked_add(other)
    }

    fn sub_or_zero(self, other: Self) -> Self {
        self.saturating_sub(other)
    }

    fn wrap(value: usize) -> Self {
        value as Self
    }
}

const _: () = assert!(usize::BITS >= u32::BITS);

// SAFETY: usize is sealed, so `to_usize` is the identity, `wrap` keeps all bits,
// `checked_add` is exact or rejects, and `sub_or_zero` saturates at zero.
unsafe impl Word for usize {
    const ZERO: Self = 0;

    fn to_usize(self) -> usize {
        self
    }

    fn checked_add(self, other: Self) -> Option<Self> {
        self.checked_add(other)
    }

    fn sub_or_zero(self, other: Self) -> Self {
        self.saturating_sub(other)
    }

    fn wrap(value: usize) -> Self {
        value
    }
}

use crate::base::cell::region;

/// Exact unused quota branded by the operation which measured it.
#[must_use = "an unused proof must settle its matching reservation"]
#[repr(transparent)]
pub struct Unused<'id> {
    amount: usize,
    token: region::Token<region::Identity<'id>>,
}

impl<'id> Unused<'id> {
    /// Seals an exact unused amount under its operation identity.
    /// # Safety
    /// `amount` must be the exact unused quota of `token`'s reservation.
    pub const unsafe fn new(amount: usize, token: region::Token<region::Identity<'id>>) -> Self {
        Self { amount, token }
    }

    pub(in crate::mem) fn into_inner(self) -> (usize, region::Token<region::Identity<'id>>) {
        (self.amount, self.token)
    }
}

const _: () = assert!(size_of::<Unused<'static>>() == size_of::<usize>());

/// Supported local reference-count integers and their nonzero representations.
pub trait Counter: Word {
    type NonZero: Copy + Debug;

    const ONE: Self;
    const TWO: Self;
    const LIVE: Self::NonZero;

    /// # Safety
    /// The value is nonzero.
    unsafe fn nonzero(self) -> Self::NonZero;

    fn get(value: Self::NonZero) -> Self;
}

impl Counter for u32 {
    type NonZero = NonZero<Self>;

    const ONE: Self = 1;
    const TWO: Self = 2;
    const LIVE: Self::NonZero = NonZero::<Self>::MIN;

    unsafe fn nonzero(self) -> Self::NonZero {
        // SAFETY: the caller proves the count is nonzero.
        unsafe { NonZero::new_unchecked(self) }
    }

    fn get(value: Self::NonZero) -> Self {
        value.get()
    }
}

impl Counter for usize {
    type NonZero = NonZero<Self>;

    const ONE: Self = 1;
    const TWO: Self = 2;
    const LIVE: Self::NonZero = NonZero::<Self>::MIN;

    unsafe fn nonzero(self) -> Self::NonZero {
        // SAFETY: the caller proves the count is nonzero.
        unsafe { NonZero::new_unchecked(self) }
    }

    fn get(value: Self::NonZero) -> Self {
        value.get()
    }
}
