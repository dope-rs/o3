use std::{mem, ptr, slice};

use crate::alloc;

/// One allocation retaining its plane initialization states and compact extent.
#[derive(Debug)]
#[repr(C)]
pub struct Block<
    E: alloc::raw::Shape,
    S,
    X = (),
    R = mem::MaybeUninit<<E as alloc::raw::Shape>::Root>,
> {
    root: ptr::NonNull<R>,
    pub(super) tail: ptr::NonNull<E::Tail>,
    pub(super) extent: E,
    pub state: S,
    extra: X,
}

/// An unpublished allocation whose planes can only be borrowed uninitialized.
pub(super) struct Uninit<E: alloc::raw::Shape> {
    memory: ptr::NonNull<u8>,
    extent: E,
}

impl<E: alloc::raw::Shape> Uninit<E> {
    pub(super) fn try_new(extent: E) -> Result<Self, alloc::Exhausted> {
        let plan = extent.layout();
        Ok(Self {
            memory: alloc::raw::LayoutExt::try_allocate(plan)?,
            extent,
        })
    }

    pub(super) fn planes_mut(&mut self) -> Arrays<'_, E::Root, E::Tail, E::Extra, E::Last> {
        let len = self.extent.len();
        // SAFETY: Shape binds all four aligned, disjoint planes to this exact
        // unpublished allocation; the exclusive owner borrow covers them all.
        unsafe {
            (
                slice::from_raw_parts_mut(self.memory.cast().as_ptr(), len),
                slice::from_raw_parts_mut(
                    self.memory.add(self.extent.offset()).cast().as_ptr(),
                    self.extent.tail_len(),
                ),
                slice::from_raw_parts_mut(
                    self.memory.add(self.extent.extra()).cast().as_ptr(),
                    len,
                ),
                slice::from_raw_parts_mut(self.memory.add(self.extent.last()).cast().as_ptr(), len),
            )
        }
    }

    /// # Safety
    /// Root has R's exact layout and initialization, Tail is initialized, and
    /// extra retains the auxiliary planes' locations and initialization states.
    pub(super) unsafe fn finish<S, X, R>(self, state: S, extra: X) -> Block<E, S, X, R> {
        // SAFETY: the retained Shape proves the exact tail offset.
        let tail = unsafe { self.memory.add(self.extent.offset()) }.cast();
        Block {
            root: self.memory.cast(),
            tail,
            extent: self.extent,
            state,
            extra,
        }
    }
}

impl<E: alloc::raw::Shape<Tail: Copy>, S> Block<E, S> {
    pub(crate) fn try_new(extent: E, state: S, value: E::Tail) -> Result<Self, alloc::Exhausted> {
        let mut memory = Uninit::try_new(extent)?;
        memory.planes_mut().1.fill(mem::MaybeUninit::new(value));
        // SAFETY: fill initialized the entire tail; root remains MaybeUninit.
        Ok(unsafe { memory.finish(state, ()) })
    }
}

impl<E: alloc::raw::Rows, S> Block<E, S> {
    pub(crate) fn try_fill(
        extent: E,
        state: S,
        init: impl Fn(usize) -> (mem::MaybeUninit<E::Root>, E::Tail),
    ) -> Result<Self, alloc::Exhausted> {
        const { assert!(!mem::needs_drop::<E::Tail>()) };
        let mut memory = Uninit::try_new(extent)?;
        let (roots, tails, _, _) = memory.planes_mut();
        alloc::raw::pair(roots, tails, init);
        // SAFETY: Rows gives equal lengths; the loop wrote every tail entry.
        Ok(unsafe { memory.finish(state, ()) })
    }
}

impl<E: alloc::raw::Rows, S> Block<E, S, (), E::Root> {
    pub(crate) fn try_init(
        extent: E,
        state: S,
        init: impl Fn(usize) -> (E::Root, E::Tail),
    ) -> Result<Self, alloc::Exhausted> {
        const { assert!(!mem::needs_drop::<E::Root>() && !mem::needs_drop::<E::Tail>()) };
        let mut memory = Uninit::try_new(extent)?;
        let (roots, tails, _, _) = memory.planes_mut();
        alloc::raw::pair(roots, tails, |index| {
            let (value, entry) = init(index);
            (mem::MaybeUninit::new(value), entry)
        });
        // SAFETY: Rows gives equal lengths; both complete planes are initialized.
        Ok(unsafe { memory.finish(state, ()) })
    }
}

impl<E: alloc::raw::Shape, S> Block<E, S, (), E::Root> {
    pub fn try_from_fn(
        extent: E,
        state: S,
        root: impl Fn(usize) -> E::Root,
        tail: impl Fn(usize) -> E::Tail,
    ) -> Result<Self, alloc::Exhausted> {
        const { assert!(!mem::needs_drop::<E::Tail>()) };
        let mut memory = Uninit::try_new(extent)?;
        let (entries, heads, _, _) = memory.planes_mut();
        alloc::raw::init(entries, &root);
        alloc::raw::init(heads, tail);
        // SAFETY: the separate loops initialized each complete plane, including
        // Shapes whose root and tail lengths differ.
        Ok(unsafe { memory.finish(state, ()) })
    }
}

impl<E: alloc::raw::Shape<Last = ()>, S> Block<E, S, alloc::raw::Initialized, E::Root> {
    /// Initializes each root and auxiliary row together, then the separate tail.
    pub fn try_planes(
        extent: E,
        state: S,
        root: impl Fn(usize) -> (E::Root, E::Extra),
        tail: impl Fn(usize) -> E::Tail,
    ) -> Result<Self, alloc::Exhausted> {
        const {
            assert!(!mem::needs_drop::<E::Tail>());
            assert!(!mem::needs_drop::<E::Extra>());
        };
        let mut memory = Uninit::try_new(extent)?;
        let (entries, heads, extra, _) = memory.planes_mut();
        alloc::raw::pair(entries, extra, |index| {
            let (value, aux) = root(index);
            (mem::MaybeUninit::new(value), aux)
        });
        alloc::raw::init(heads, tail);
        // SAFETY: all three complete planes are initialized; the marker retains
        // auxiliary initialization while its location is reconstructed from Shape.
        Ok(unsafe { memory.finish(state, alloc::raw::Initialized) })
    }

    pub fn values(&self) -> &[E::Extra] {
        // SAFETY: Shape retains this initialized plane's complete allocation offset.
        let extra = unsafe { self.as_ptr().byte_add(self.extent.extra()).cast() };
        // SAFETY: the initialized plane has the exact root length and owner borrow.
        unsafe { slice::from_raw_parts(extra, self.extent.len()) }
    }
}

impl<E: alloc::raw::Shape, S> Block<E, S> {
    pub(crate) fn enumerate(
        self,
        init: impl Fn(usize) -> E::Extra,
    ) -> Block<E, S, ptr::NonNull<E::Extra>> {
        const { assert!(!mem::needs_drop::<E::Extra>()) };
        // SAFETY: Shape proves the aligned, disjoint third plane. This owned
        // Block has not published that plane before this initialization.
        let extra = unsafe { self.root.cast::<u8>().add(self.extent.extra()) }
            .cast::<mem::MaybeUninit<E::Extra>>();
        let entries = unsafe { slice::from_raw_parts_mut(extra.as_ptr(), self.extent.len()) };
        alloc::raw::init(entries, init);
        let this = mem::ManuallyDrop::new(self);
        Block {
            root: this.root,
            tail: this.tail,
            // SAFETY: ManuallyDrop transfers every owned field exactly once;
            // the completed initialization admits the typed third-plane view.
            extent: unsafe { ptr::read(&this.extent) },
            state: unsafe { ptr::read(&this.state) },
            extra: extra.cast(),
        }
    }
}

impl<E: alloc::raw::Rows<Tail = mem::MaybeUninit<T>>, S, T>
    Block<E, S, (ptr::NonNull<E::Extra>, ptr::NonNull<E::Last>)>
{
    pub fn try_parts(
        extent: E,
        state: S,
        init: impl Fn(usize) -> (E::Extra, E::Last),
    ) -> Result<Self, alloc::Exhausted> {
        const {
            assert!(!mem::needs_drop::<E::Extra>());
            assert!(!mem::needs_drop::<E::Last>());
        };
        let mut memory = Uninit::try_new(extent)?;
        let (_, _, extra, last) = memory.planes_mut();
        alloc::raw::pair(extra, last, |index| {
            let (entry, value) = init(index);
            (mem::MaybeUninit::new(entry), value)
        });
        let extra = (
            ptr::NonNull::from(extra).cast(),
            ptr::NonNull::from(last).cast(),
        );
        // SAFETY: both equal-length auxiliary planes were fully initialized;
        // root and tail remain MaybeUninit and the pointers retain their owner.
        Ok(unsafe { memory.finish(state, extra) })
    }
}

impl<E: alloc::raw::Shape, S, X, R> Drop for Block<E, S, X, R> {
    fn drop(&mut self) {
        // SAFETY: this owner drops its complete initialized root plane before
        // releasing the exact allocation retained by its immutable Shape.
        unsafe {
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(
                self.root.as_ptr(),
                self.extent.len(),
            ));
            alloc::raw::LayoutExt::deallocate(self.extent.layout(), self.root);
        }
    }
}

impl<E: alloc::raw::Shape, S, X, R> Block<E, S, X, R> {
    /// The root allocation pointer without borrowing its elements.
    pub(crate) const fn as_ptr(&self) -> *mut R {
        self.root.as_ptr()
    }

    pub const fn extent(&self) -> &E {
        &self.extent
    }

    pub fn tail(&self) -> &[E::Tail] {
        // SAFETY: every constructor initializes exactly `tail_len` Tail values
        // at the aligned disjoint offset retained by this Shape, and the shared
        // borrow keeps the owning allocation live.
        unsafe { slice::from_raw_parts(self.tail.as_ptr(), self.extent.tail_len()) }
    }

    /// Exclusively borrows the inline state and both disjoint initialized planes.
    pub fn parts_mut(&mut self) -> (&mut S, &mut [R], &mut [E::Tail]) {
        // SAFETY: construction retains initialized R and Tail planes in Shape's
        // disjoint allocation, separate from the exclusively borrowed inline state.
        unsafe {
            (
                &mut self.state,
                slice::from_raw_parts_mut(self.root.as_ptr(), self.extent.len()),
                slice::from_raw_parts_mut(self.tail.as_ptr(), self.extent.tail_len()),
            )
        }
    }
}

impl<E: alloc::raw::Shape, S, X, R> AsRef<[R]> for Block<E, S, X, R> {
    fn as_ref(&self) -> &[R] {
        // SAFETY: construction initializes exactly `len` R values at `root` in
        // the Shape's first aligned, disjoint plane, and the shared borrow keeps
        // them initialized for its whole duration.
        unsafe { slice::from_raw_parts(self.root.as_ptr(), self.extent.len()) }
    }
}

impl<E: alloc::raw::Shape, S, X, R> AsMut<[R]> for Block<E, S, X, R> {
    fn as_mut(&mut self) -> &mut [R] {
        self.parts_mut().1
    }
}

impl<E: alloc::raw::Shape, S> Block<E, S, ptr::NonNull<E::Extra>, mem::MaybeUninit<E::Root>> {
    pub(crate) fn values(&self) -> &[E::Extra] {
        // SAFETY: the `NonNull<E::Extra>` state is produced only by `enumerate`,
        // which initialized exactly `len` Extra values at this aligned disjoint
        // plane address; the shared borrow keeps both plane and owner live.
        unsafe { slice::from_raw_parts(self.extra.as_ptr(), self.extent.len()) }
    }

    pub(crate) fn values_mut(&mut self) -> &mut [E::Extra] {
        // SAFETY: the `NonNull<E::Extra>` state proves the same initialized
        // `len`-element plane as `values`; the exclusive borrow additionally
        // excludes every other access to it.
        unsafe { slice::from_raw_parts_mut(self.extra.as_ptr(), self.extent.len()) }
    }
}

impl<E: alloc::raw::Rows, S>
    Block<E, S, (ptr::NonNull<E::Extra>, ptr::NonNull<E::Last>), mem::MaybeUninit<E::Root>>
{
    pub fn extra(&self) -> (&[E::Extra], &[E::Last]) {
        // SAFETY: the `(NonNull<E::Extra>, NonNull<E::Last>)` state is produced
        // only by `try_parts`, which initialized both equal-length auxiliary
        // planes through Rows at these disjoint Shape offsets; the shared borrow
        // keeps the owning allocation and both planes live.
        unsafe {
            (
                slice::from_raw_parts(self.extra.0.as_ptr(), self.extent.len()),
                slice::from_raw_parts(self.extra.1.as_ptr(), self.extent.len()),
            )
        }
    }

    pub fn extra_mut(&mut self) -> (&mut [E::Extra], &mut [E::Last]) {
        // SAFETY: Shape proves both initialized auxiliary planes are disjoint
        // and each has exactly len entries in this exclusive allocation borrow.
        unsafe {
            (
                slice::from_raw_parts_mut(self.extra.0.as_ptr(), self.extent.len()),
                slice::from_raw_parts_mut(self.extra.1.as_ptr(), self.extent.len()),
            )
        }
    }
}

type Arrays<'a, A, B, C, D> = (
    &'a mut [mem::MaybeUninit<A>],
    &'a mut [mem::MaybeUninit<B>],
    &'a mut [mem::MaybeUninit<C>],
    &'a mut [mem::MaybeUninit<D>],
);
