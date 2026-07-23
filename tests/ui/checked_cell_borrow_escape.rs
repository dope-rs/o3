use o3::cell::CheckedCell;

fn escape(cell: &CheckedCell<u8>) -> &mut u8 {
    cell.with_mut(|value| value)
}

fn main() {}
