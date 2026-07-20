use crate::buffer::{CapacityError, SpareWriter};
use crate::marker::ThreadBound;
use std::cell::Cell;
use std::marker::{PhantomData, PhantomPinned};
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr;
use std::ptr::NonNull;
use std::slice;

pub struct Pool {
    bytes: Box<[Cell<MaybeUninit<u8>>]>,
    capacity: u32,
    free: Box<[Cell<u32>]>,
    free_len: Cell<u32>,
    _pin: PhantomPinned,
    _thread: ThreadBound,
}

impl Pool {
    pub fn new(slots: usize, capacity: usize) -> Self {
        assert!(capacity > 0, "buffer pool needs capacity");
        assert!(u32::try_from(slots).is_ok(), "buffer pool slot overflow");
        assert!(
            u32::try_from(capacity).is_ok(),
            "buffer pool capacity overflow"
        );
        let total = slots
            .checked_mul(capacity)
            .expect("buffer pool capacity overflow");
        Self {
            bytes: (0..total)
                .map(|_| Cell::new(MaybeUninit::uninit()))
                .collect(),
            capacity: capacity as u32,
            free: (0..slots as u32).map(Cell::new).collect(),
            free_len: Cell::new(slots as u32),
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
        let offset = index as usize * this.capacity as usize;
        Some(Lease {
            pool: NonNull::from(this),
            data: unsafe {
                NonNull::new_unchecked(this.bytes.as_ptr().add(offset) as *mut Cell<MaybeUninit<u8>>)
            },
            capacity: this.capacity,
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
    data: NonNull<Cell<MaybeUninit<u8>>>,
    capacity: u32,
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
        self.capacity as usize
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
        let mut additional = 0usize;
        for slice in &src {
            let Some(next) = additional.checked_add(slice.len()) else {
                return Err(CapacityError::new(usize::MAX, self.capacity()));
            };
            additional = next;
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
        let attempted = self
            .len()
            .checked_add(additional)
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if attempted > capacity {
            return Err(CapacityError::new(attempted, capacity));
        }
        if additional > (self.capacity - self.tail) as usize {
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

    pub fn contiguous_spare_writer(&mut self) -> SpareWriter<'_> {
        let ptr = unsafe {
            self.data
                .as_ptr()
                .add(self.tail as usize)
                .cast::<MaybeUninit<u8>>()
        };
        unsafe { SpareWriter::new(ptr, (self.capacity - self.tail) as usize, &mut self.tail) }
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
