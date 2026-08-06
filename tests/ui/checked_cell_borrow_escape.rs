use o3::cell::Checked;

fn escape(cell: &Checked<u8>) -> &mut u8 {
    cell.with_mut(|value| value)
}

fn main() {}
