use crate::buffer::{CapacityError, SpareWriter};
use crate::marker::ThreadBound;
use std::cell::Cell;
use std::error::Error;
use std::fmt;
use std::marker::{PhantomData, PhantomPinned};
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
use std::pin::Pin;
use std::ptr;
use std::ptr::NonNull;
use std::slice;

type ByteCell = Cell<MaybeUninit<u8>>;

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
        (0..layout.slots).map(Cell::new).collect(),
    )
}

pub struct Pool {
    bytes: Box<[ByteCell]>,
    free: Box<[Cell<u32>]>,
    free_len: Cell<u32>,
    capacity: NonZeroU32,
    _pin: PhantomPinned,
    _thread: ThreadBound,
}

impl Pool {
    pub fn new(layout: PoolLayout) -> Self {
        let (bytes, free) = allocate(layout);
        Self {
            bytes,
            free,
            free_len: Cell::new(layout.slots),
            capacity: layout.capacity,
            _pin: PhantomPinned,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn try_acquire(self: Pin<&Self>) -> Option<Lease<'_>> {
        let this = self.get_ref();
        let len = this.free_len.get();
        if len == 0 {
            return None;
        }
        let index = unsafe { this.free.get_unchecked(len as usize - 1) }.get();
        this.free_len.set(len - 1);
        let offset = index as usize * this.capacity.get() as usize;
        Some(Lease {
            pool: NonNull::from(this),
            data: unsafe {
                NonNull::new_unchecked(this.bytes.as_ptr().add(offset) as *mut ByteCell)
            },
            index,
            head: 0,
            tail: 0,
            lifetime: PhantomData,
        })
    }

    pub fn available(&self) -> usize {
        self.free_len.get() as usize
    }

    fn release(&self, index: u32) {
        let len = self.free_len.get();
        unsafe { self.free.get_unchecked(len as usize) }.set(index);
        self.free_len.set(len + 1);
    }
}

pub struct Lease<'d> {
    pool: NonNull<Pool>,
    data: NonNull<ByteCell>,
    index: u32,
    head: u32,
    tail: u32,
    lifetime: PhantomData<&'d Pool>,
}

impl Lease<'_> {
    fn pool(&self) -> &Pool {
        unsafe { self.pool.as_ref() }
    }

    pub fn len(&self) -> usize {
        (self.tail - self.head) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn capacity(&self) -> usize {
        self.pool().capacity.get() as usize
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
        let capacity = self.capacity();
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
        let capacity = self.capacity();
        let len = self.len();
        let attempted = len
            .checked_add(additional)
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if attempted > capacity {
            return Err(CapacityError::new(attempted, capacity));
        }
        if additional > (self.pool().capacity.get() - self.tail) as usize {
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
        let remaining = (self.pool().capacity.get() - self.tail) as usize;
        let ptr = unsafe {
            self.data
                .as_ptr()
                .add(self.tail as usize)
                .cast::<MaybeUninit<u8>>()
        };
        unsafe { SpareWriter::new(ptr, remaining, &mut self.tail) }
    }

    pub fn consume(&mut self, amount: usize) {
        assert!(amount <= self.len(), "buffer pool lease consume overflow");
        unsafe { super::consume(&mut self.head, &mut self.tail, amount) };
    }

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

impl AsRef<[u8]> for Lease<'_> {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.pool().release(self.index);
    }
}
