use o3::buffer::{self, hold};

struct Source;

impl buffer::Source for Source {
    type View<'a> = &'a [u8];

    fn remaining(&self) -> usize {
        0
    }
}

impl<'d> hold::raw::Retain<'d> for Source {}

fn main() {}
