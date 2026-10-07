use o3::buffer::bytes::scope;

fn wrong_source<'a, 'first, 'second, P: scope::raw::Select<'first>>(
    source: scope::View<'a, 'second>,
    proof: P,
) {
    let _ = source.get(proof);
}

fn main() {}
