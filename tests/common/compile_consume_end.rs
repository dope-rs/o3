use o3::buffer::{self, write::consume::scope::{self, Input}};
use std::num::NonZero;

fn wrong_source<R: buffer::Cursor + buffer::raw::Consume>(left: &R, right: &mut R) -> Result<(), scope::Error> {
    Input::new(left).scope(|input| {
        let (_, end) = input.try_prefix::<1>(NonZero::<usize>::MIN)?;
        Input::consume::<_, 1>(right, |_| (Some(end), ()));
        Ok(())
    })
}

fn main() {}
