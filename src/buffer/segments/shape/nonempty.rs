use crate::{
    buffer,
    buffer::segments::{Item, Many, Shape},
};

/// Physical owners proved to contain at least one byte.
#[must_use = "consume or release these owners"]
pub enum Nonempty<T: buffer::raw::Segment, S: buffer::raw::Unary<T> = Item<T>> {
    One(S),
    Many(Many<T>),
}

impl<T: buffer::raw::Segment> Nonempty<T> {
    pub fn into_shape(self) -> Shape<T> {
        match self {
            Self::One(owner) => Shape::One(owner),
            Self::Many(owners) => Shape::Many(owners),
        }
    }

    pub fn front(&self) -> &T {
        match self {
            Self::One(owner) => owner.as_ref(),
            Self::Many(owners) => owners.front(),
        }
    }

    pub fn is_single(&self) -> bool {
        matches!(self, Self::One(_))
    }
}

const _: () = {
    use crate::buffer::resident;
    assert!(size_of::<Nonempty<resident::Shared<'static>>>() == 4 * size_of::<usize>());
    assert!(
        size_of::<Option<Nonempty<resident::Shared<'static>>>>()
            == size_of::<Nonempty<resident::Shared<'static>>>()
    );
};
