use crate::depot::heap::indexed::resident::{self, Sealed};

#[derive(Debug)]
pub struct Scheduled;
#[derive(Debug)]
pub struct Idle;
#[derive(Debug)]
pub struct Control;
#[derive(Debug)]
pub struct Data;
#[derive(Debug)]
pub struct Active;

impl Sealed for Scheduled {
    const TAG: resident::raw::Tag = resident::raw::Tag::Common;
}

impl<T: resident::Values> resident::Phase<T> for Scheduled {
    type Item = T::Scheduled;
    fn value(value: Self::Item) -> resident::Owned<T> {
        resident::Value::Scheduled(value)
    }
}

impl Sealed for Idle {
    const TAG: resident::raw::Tag = resident::raw::Tag::Common;
}

impl<T: resident::Values> resident::Phase<T> for Idle {
    type Item = T::Idle;
    fn value(value: Self::Item) -> resident::Owned<T> {
        resident::Value::Idle(value)
    }
}

impl Sealed for Control {
    const TAG: resident::raw::Tag = resident::raw::Tag::Control;
}

impl<T: resident::Values> resident::Phase<T> for Control {
    type Item = T::Control;
    fn value(value: Self::Item) -> resident::Owned<T> {
        resident::Value::Control(value)
    }
}

impl Sealed for Data {
    const TAG: resident::raw::Tag = resident::raw::Tag::Data;
}

impl<T: resident::Values> resident::Phase<T> for Data {
    type Item = T::Data;
    fn value(value: Self::Item) -> resident::Owned<T> {
        resident::Value::Data(value)
    }
}

impl Sealed for Active {
    const TAG: resident::raw::Tag = resident::raw::Tag::Active;
}

impl<T: resident::Values> resident::Phase<T> for Active {
    type Item = T::Active;
    fn value(value: Self::Item) -> resident::Owned<T> {
        resident::Value::Active(value)
    }
}
