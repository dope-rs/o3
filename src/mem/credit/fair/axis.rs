use std::mem;

/// One dimension in a fixed fair-credit domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Axis<const N: usize>(usize);

impl<const N: usize> Axis<N> {
    pub const ALL: [Self; N] = {
        assert!(N != 0);
        let mut axes = [Self(0); N];
        let mut index = 0;
        while index < N {
            axes[index] = Self(index);
            index += 1;
        }
        axes
    };

    pub const fn at<const I: usize>() -> Self {
        const { assert!(I < N) };
        Self(I)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

const _: () = assert!(mem::size_of::<Axis<1>>() == mem::size_of::<usize>());
