use std::{marker, mem};

use crate::depot::array::{Len, copy, prefix::Prefix};

const _: () = {
    assert!(mem::size_of::<copy::IntoIter<u8, 4>>() == 12);
    assert!(mem::size_of::<copy::IntoIter<u64, 4>>() == 40);
    assert!(mem::align_of::<copy::IntoIter<u64, 4>>() == 8);
};

pub(super) struct Compact<'a, T: Copy, const N: usize> {
    source: *const T,
    target: *mut mem::MaybeUninit<T>,
    len: &'a mut Len<N>,
    end: Len<N>,
    storage: marker::PhantomData<&'a mut [mem::MaybeUninit<T>; N]>,
}

impl<'a, T: Copy, const N: usize> Compact<'a, T, N> {
    pub(super) fn retain(target: &'a mut Prefix<T, N>) -> Self {
        let pointer = target.entries.as_mut_ptr();
        Self {
            source: pointer.cast(),
            target: pointer,
            end: target.len,
            len: &mut target.len,
            storage: marker::PhantomData,
        }
    }

    pub(super) fn from_source(target: &'a mut Prefix<T, N>, source: &'a Prefix<T, N>) -> Self {
        Self {
            source: source.entries.as_ptr().cast(),
            target: target.entries.as_mut_ptr(),
            end: source.len,
            len: &mut target.len,
            storage: marker::PhantomData,
        }
    }

    pub(super) fn filter(self, mut keep: impl FnMut(&T) -> bool) {
        let mut written = 0;
        for read in 0..self.end.get() {
            // SAFETY: both constructors retain this exact initialized prefix.
            let source = unsafe { &*self.source.add(read) };
            if keep(source) {
                let value = *source;
                // SAFETY: at most one write per read keeps written <= read < end <= N.
                // The source borrow ends before this in-place write.
                unsafe {
                    self.target
                        .add(written as usize)
                        .write(mem::MaybeUninit::new(value))
                };
                written += 1;
            }
        }
        *self.len = Len(written);
    }
}
