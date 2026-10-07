use o3::buffer::pool::Layout;

fn main() {
    assert_eq!(Layout::constant::<1, 0>().slots(), 1);
}
