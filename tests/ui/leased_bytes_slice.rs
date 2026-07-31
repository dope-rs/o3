use o3::buffer::{Bytes, Leased};

fn slice(bytes: Bytes<Leased>) {
    let _ = bytes.get(0..0);
}

fn main() {}
