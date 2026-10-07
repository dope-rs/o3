use crate::{
    buffer,
    buffer::segments::{Item, Many, Shape},
    mem::rc,
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

    pub fn try_pop(self, limit: usize) -> Result<(T, Shape<T>), (rc::Error, Self)> {
        match self {
            Self::One(owner) => {
                let (head, rest) = owner
                    .try_split(limit)
                    .map_err(|(error, owner)| (error, Self::One(owner)))?;
                Ok((head, rest.map_or(Shape::Empty, Shape::One)))
            }
            Self::Many(owners) => {
                let (head, rest) = owners
                    .try_pop(limit)
                    .map_err(|(error, owners)| (error, Self::Many(owners)))?;
                Ok((head, rest.into_shape()))
            }
        }
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
