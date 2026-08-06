use o3::buffer::{PrefixConsumer, PrefixLength, PrefixProof};

struct Cursor {
    len: usize,
}

impl PrefixLength for Cursor {
    fn prefix_len(&self) -> usize {
        self.len
    }
}

impl PrefixConsumer for Cursor {
    fn consume_validated_prefix(&mut self, proof: PrefixProof) {
        self.len -= proof.amount();
    }
}

fn main() {
    let mut cursor = Cursor { len: 8 };
    let prefix = cursor.try_consume_prefix(4).unwrap();
    cursor.len = 2;
    prefix.commit();
}
