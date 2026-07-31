use crate::buffer::{CapacityError, PrefixLength, SpareWriter};
use crate::marker::ThreadBound;
use std::cell::Cell;
use std::error::Error;
use std::fmt;
use std::iter;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
use std::ptr;
use std::ptr::NonNull;
use std::slice;

pub mod shared;

type ByteCell = Cell<MaybeUninit<u8>>;

mod private {
    pub trait Sealed {}
}

#[doc(hidden)]
pub trait PoolCapacity: private::Sealed + Copy {
    fn get(&self) -> u32;
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct RuntimePoolCapacity(NonZeroU32);

impl private::Sealed for RuntimePoolCapacity {}

impl PoolCapacity for RuntimePoolCapacity {
    fn get(&self) -> u32 {
        self.0.get()
    }
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct FixedPoolCapacity<const CAP: u32>;

impl<const CAP: u32> private::Sealed for FixedPoolCapacity<CAP> {}

impl<const CAP: u32> PoolCapacity for FixedPoolCapacity<CAP> {
    fn get(&self) -> u32 {
        CAP
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolLayout {
    slots: u32,
    capacity: NonZeroU32,
    total: usize,
}

impl PoolLayout {
    pub const fn new(slots: u32, capacity: u32) -> Result<Self, PoolLayoutError> {
        let Some(capacity) = NonZeroU32::new(capacity) else {
            return Err(PoolLayoutError::ZeroCapacity);
        };
        let Some(total) = (slots as usize).checked_mul(capacity.get() as usize) else {
            return Err(PoolLayoutError::CapacityOverflow);
        };
        if total > isize::MAX as usize {
            return Err(PoolLayoutError::CapacityOverflow);
        }
        Ok(Self {
            slots,
            capacity,
            total,
        })
    }

    pub const fn slots(self) -> u32 {
        self.slots
    }

    pub const fn capacity(self) -> u32 {
        self.capacity.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolLayoutError {
    ZeroCapacity,
    SlotOverflow,
    CapacityOverflow,
}

impl fmt::Display for PoolLayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => f.write_str("buffer pool capacity must be positive"),
            Self::SlotOverflow => f.write_str("buffer pool slot count overflow"),
            Self::CapacityOverflow => f.write_str("buffer pool allocation size overflow"),
        }
    }
}

impl Error for PoolLayoutError {}

fn allocate(layout: PoolLayout) -> (Box<[ByteCell]>, Box<[Cell<u32>]>) {
    (
        (0..layout.total)
            .map(|_| Cell::new(MaybeUninit::uninit()))
            .collect(),
        iter::once(Cell::new(layout.slots))
            .chain((0..layout.slots).map(Cell::new))
            .collect(),
    )
}

pub struct Pool<C = RuntimePoolCapacity> {
    bytes: Box<[ByteCell]>,
    free: Box<[Cell<u32>]>,
    capacity: C,
    _thread: ThreadBound,
}

impl Pool<RuntimePoolCapacity> {
    pub fn from_layout(layout: PoolLayout) -> Self {
        let (bytes, free) = allocate(layout);
        Self {
            bytes,
            free,
            capacity: RuntimePoolCapacity(layout.capacity),
            _thread: ThreadBound::NEW,
        }
    }
}

impl<const CAP: u32> Pool<FixedPoolCapacity<CAP>> {
    const VALID: () = {
        assert!(CAP != 0, "fixed buffer pool capacity must be positive");
        assert!(
            CAP as usize <= isize::MAX as usize / u32::MAX as usize,
            "fixed buffer pool capacity exceeds the allocation limit"
        );
    };

    pub const CAPACITY: usize = CAP as usize;

    pub fn new(slots: u32) -> Self {
        let () = Self::VALID;
        let layout = PoolLayout {
            slots,
            // SAFETY: `Self::VALID` rejects zero capacities at compile time.
            capacity: unsafe { NonZeroU32::new_unchecked(CAP) },
            total: slots as usize * CAP as usize,
        };
        let (bytes, free) = allocate(layout);
        Self {
            bytes,
            free,
            capacity: FixedPoolCapacity,
            _thread: ThreadBound::NEW,
        }
    }
}

impl<C: PoolCapacity> Pool<C> {
    pub fn try_acquire(&self) -> Option<Lease<'_, C>> {
        let control = unsafe { self.free.get_unchecked(0) };
        let len = control.get();
        if len == 0 {
            return None;
        }
        let index = unsafe { self.free.get_unchecked(len as usize) }.get();
        control.set(len - 1);
        let offset = index as usize * self.capacity.get() as usize;
        Some(Lease {
            free: NonNull::from(control),
            data: unsafe {
                NonNull::new_unchecked(self.bytes.as_ptr().add(offset) as *mut ByteCell)
            },
            capacity: self.capacity,
            index,
            head: 0,
            tail: 0,
            lifetime: PhantomData,
        })
    }

    pub fn available(&self) -> usize {
        unsafe { self.free.get_unchecked(0) }.get() as usize
    }
}

pub struct Lease<'d, C: PoolCapacity = RuntimePoolCapacity> {
    free: NonNull<Cell<u32>>,
    data: NonNull<ByteCell>,
    capacity: C,
    index: u32,
    head: u32,
    tail: u32,
    lifetime: PhantomData<&'d Pool<C>>,
}

impl<C: PoolCapacity> Lease<'_, C> {
    pub fn len(&self) -> usize {
        (self.tail - self.head) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    fn pool_capacity(&self) -> usize {
        self.capacity.get() as usize
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        let start = self.reserve_append(1)?;
        unsafe { (*self.data.as_ptr().add(start)).set(MaybeUninit::new(byte)) };
        self.tail = start as u32 + 1;
        Ok(())
    }

    pub fn try_extend_from_slice(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        let start = self.reserve_append(src.len())?;
        unsafe {
            ptr::copy_nonoverlapping(
                src.as_ptr(),
                self.data.as_ptr().add(start).cast(),
                src.len(),
            );
        }
        self.tail = (start + src.len()) as u32;
        Ok(())
    }

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        src: [&[u8]; N],
    ) -> Result<(), CapacityError> {
        let capacity = self.pool_capacity();
        let mut additional = 0usize;
        for slice in &src {
            additional = additional
                .checked_add(slice.len())
                .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
            if additional > capacity {
                return Err(CapacityError::new(additional, capacity));
            }
        }
        let start = self.reserve_append(additional)?;
        let mut offset = start;
        for slice in src {
            unsafe {
                ptr::copy_nonoverlapping(
                    slice.as_ptr(),
                    self.data.as_ptr().add(offset).cast(),
                    slice.len(),
                );
            }
            offset += slice.len();
        }
        self.tail = offset as u32;
        Ok(())
    }

    fn reserve_append(&mut self, additional: usize) -> Result<usize, CapacityError> {
        let capacity = self.pool_capacity();
        let len = self.len();
        let attempted = len
            .checked_add(additional)
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if attempted > capacity {
            return Err(CapacityError::new(attempted, capacity));
        }
        if additional > (self.capacity.get() - self.tail) as usize {
            self.compact();
        }
        Ok(self.tail as usize)
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self.data.as_ptr().add(self.head as usize).cast(),
                self.len(),
            )
        }
    }

    pub fn spare_writer(&mut self) -> SpareWriter<'_> {
        if self.head != 0 {
            self.compact();
        }
        self.contiguous_spare_writer()
    }

    fn contiguous_spare_writer(&mut self) -> SpareWriter<'_> {
        let remaining = (self.capacity.get() - self.tail) as usize;
        let ptr = unsafe {
            self.data
                .as_ptr()
                .add(self.tail as usize)
                .cast::<MaybeUninit<u8>>()
        };
        unsafe { SpareWriter::new(ptr, remaining, &mut self.tail) }
    }

    pub fn try_consume(&mut self, amount: usize) -> Result<(), CapacityError> {
        let len = self.len();
        if amount > len {
            return Err(CapacityError::new(amount, len));
        }
        self.consume_valid(amount);
        Ok(())
    }

    fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        unsafe { super::consume(&mut self.head, &mut self.tail, amount) };
    }

    super::prefix::consume_prefix_api!(Self::consume_valid);

    pub fn truncate(&mut self, len: usize) {
        if len >= self.len() {
            return;
        }
        self.tail = self.head + len as u32;
        if self.head == self.tail {
            self.head = 0;
            self.tail = 0;
        }
    }

    #[cold]
    fn compact(&mut self) {
        unsafe { super::compact(self.data.as_ptr().cast(), &mut self.head, &mut self.tail) };
    }

    pub fn as_ptr(&self) -> *const u8 {
        unsafe { self.data.as_ptr().add(self.head as usize).cast() }
    }
}

impl Lease<'_, RuntimePoolCapacity> {
    pub fn capacity(&self) -> usize {
        self.pool_capacity()
    }
}

impl<const CAP: u32> Lease<'_, FixedPoolCapacity<CAP>> {
    pub const fn capacity(&self) -> usize {
        CAP as usize
    }
}

impl<C: PoolCapacity> AsRef<[u8]> for Lease<'_, C> {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl<C: PoolCapacity> PrefixLength for Lease<'_, C> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<C: PoolCapacity> Drop for Lease<'_, C> {
    fn drop(&mut self) {
        // SAFETY: the lease lifetime keeps the pool, and therefore its free-list
        // allocation, alive. Cell zero stores the current length; the following
        // cells store every free slot index exactly once.
        unsafe {
            let control = self.free.as_ref();
            let len = control.get();
            self.free
                .as_ptr()
                .add(len as usize + 1)
                .as_ref()
                .unwrap_unchecked()
                .set(self.index);
            control.set(len + 1);
        }
    }
}
