use std::io;
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

core::cfg_select! {
    target_os = "linux" => {
        const MAP_FLAGS: libc::c_int =
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE;

        fn madvise_hugepage(ptr: *mut libc::c_void, bytes: usize) -> io::Result<()> {
            let rc = unsafe { libc::madvise(ptr, bytes, libc::MADV_HUGEPAGE) };
            if rc < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
        }
    }
    _ => {
        const MAP_FLAGS: libc::c_int = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS;

        fn madvise_hugepage(_ptr: *mut libc::c_void, _bytes: usize) -> io::Result<()> {
            Ok(())
        }
    }
}

pub unsafe trait ZeroValid {}

macro_rules! impl_zero_valid {
    ($($t:ty),* $(,)?) => { $(unsafe impl ZeroValid for $t {})* };
}

impl_zero_valid!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

#[inline]
pub fn boxed_zeroed<T: ZeroValid>() -> Box<T> {
    // SAFETY: `T: ZeroValid` — an all-zero `T` is a valid value.
    unsafe { Box::new_zeroed().assume_init() }
}

pub struct Mmap<T> {
    ptr: NonNull<T>,
    len: usize,
    bytes: usize,
    _marker: PhantomData<T>,
}

impl<T> Mmap<T> {
    pub fn new_zeroed(len: usize) -> io::Result<Self>
    where
        T: ZeroValid,
    {
        assert!(len > 0);
        let bytes = size_of::<T>().checked_mul(len).expect("slab size overflow");
        let prot = libc::PROT_READ | libc::PROT_WRITE;
        let raw = unsafe { libc::mmap(std::ptr::null_mut(), bytes, prot, MAP_FLAGS, -1, 0) };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let _ = madvise_hugepage(raw, bytes);
        let ptr = NonNull::new(raw as *mut T).expect("mmap returned non-null");
        Ok(Self {
            ptr,
            len,
            bytes,
            _marker: PhantomData,
        })
    }

    pub fn prewarm(&mut self) {
        if self.bytes == 0 {
            return;
        }
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let page = if page == 0 { 4096 } else { page };
        let base = self.ptr.as_ptr() as *mut u8;
        let mut off = 0;
        while off < self.bytes {
            unsafe { base.add(off).write_volatile(0) };
            off += page;
        }
    }
}

impl<T> Deref for Mmap<T> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl<T> DerefMut for Mmap<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl<T> Drop for Mmap<T> {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.bytes);
        }
    }
}
