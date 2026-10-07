use o3::depot::queue::pool::fair;

fn escape<'a, 'id>(owner: &'a fair::Lanes<'id, u8>) -> Option<&'a u8> {
    owner.inner.get(0)?.front()
}

fn main() {}
