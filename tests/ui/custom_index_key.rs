use o3::collections::IndexKey;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Custom(usize);

impl IndexKey for Custom {
    fn index(self) -> usize {
        self.0
    }
}

fn main() {}
