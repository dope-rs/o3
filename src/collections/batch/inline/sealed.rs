use std::mem;

#[repr(transparent)]
pub(crate) struct Slots<T, const N: usize>([mem::MaybeUninit<T>; N]);

impl<T, const N: usize> Slots<T, N> {
    pub(super) const fn new() -> Self {
        Self([const { mem::MaybeUninit::uninit() }; N])
    }

    pub(super) fn write(&mut self, index: usize, value: T) {
        self.0[index].write(value);
    }

    pub(super) fn take(&mut self, index: usize) -> T {
        unsafe { self.0[index].assume_init_read() }
    }
}
