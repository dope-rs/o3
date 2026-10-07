use o3::depot::arena::slab::{self, entries, key};

fn escape(
    vacant: slab::Vacant<entries::Pending<'_, u8, { u32::MAX }, entries::Interior>>,
) -> key::Handle {
    let key = vacant.key();
    drop(vacant);
    key
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Custom(u32);

impl slab::external::Generation for Custom {
    const INITIAL: Self = Self(0);

    fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

fn main() {}
