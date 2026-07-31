use o3::buffer::{PrefixLength, ValidatedPrefix};

struct Cursor {
    len: usize,
}

impl PrefixLength for Cursor {
    fn prefix_len(&self) -> usize {
        self.len
    }
}

fn commit(cursor: &mut Cursor, amount: usize) {
    cursor.len -= amount;
}

fn main() {
    let mut cursor = Cursor { len: 8 };
    let prefix = ValidatedPrefix::try_new(&mut cursor, 4, commit).unwrap();
    cursor.len = 2;
    prefix.commit();
}
