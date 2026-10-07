use crate::{
    buffer,
    buffer::segments::{
        self, Chain, Handle, Header, Pool,
        chain::{edit, view},
        handles,
    },
};

/// An exclusive, nonempty front whose owner can be inspected or taken.
pub struct Front<'a, T, H: Handle<T> = Pool<T>> {
    pub(in crate::buffer::segments::chain) sequence: &'a mut Chain<T, H>,
}

impl<'a, T: buffer::raw::Segment, H: Handle<T>> Front<'a, T, H> {
    pub(in crate::buffer::segments::chain) fn new(sequence: &'a mut Chain<T, H>) -> Option<Self> {
        if sequence.key.is_empty() {
            return None;
        }
        Some(Self { sequence })
    }

    pub fn get(&self) -> &T {
        // SAFETY: Front retains the original exclusive nonempty sequence loan.
        match unsafe { self.sequence.occupied() } {
            view::Occupied::Single(_, item) => item,
            view::Occupied::Multi(desc) => {
                // SAFETY: this retained descriptor owns its initialized front in this exact store.
                unsafe { handles::nodes(&self.sequence.handle).value(desc.links.head_raw()) }
            }
        }
        .as_ref()
    }

    pub(in crate::buffer::segments::chain) fn position(&self) -> handles::Occupied {
        // SAFETY: every Front constructor retains exclusive ownership of a nonempty sequence.
        unsafe { self.sequence.key.occupied() }
    }

    pub fn pop(self) -> T {
        self.take().into_inner()
    }

    /// Removes and returns the admitted nonempty front owner.
    pub fn take(self) -> segments::Item<T> {
        match self.position() {
            handles::Occupied::Single(index) => edit::Single {
                sequence: self.sequence,
                index,
            }
            .take(),
            handles::Occupied::Multi(index) => edit::Multi {
                sequence: self.sequence,
                index,
            }
            .take(),
        }
    }
}

const _: () = {
    assert!(size_of::<Front<'static, (), Pool<()>>>() == size_of::<usize>());
    assert!(size_of::<Option<Front<'static, (), Pool<()>>>>() == size_of::<usize>());
    assert!(size_of::<Front<'static, (), &Header<()>>>() == size_of::<usize>());
    assert!(size_of::<Option<Front<'static, (), &Header<()>>>>() == size_of::<usize>());
};
