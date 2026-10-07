use crate::depot::heap::indexed::resident;

pub trait Sealed {
    const TAG: resident::raw::Tag;
}
