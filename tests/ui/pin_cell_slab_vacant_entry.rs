use o3::collections::PinCellSlabVacantEntry;

fn duplicate<'a>(
    entry: PinCellSlabVacantEntry<'a, u8>,
) -> (
    PinCellSlabVacantEntry<'a, u8>,
    PinCellSlabVacantEntry<'a, u8>,
) {
    (entry, entry)
}

fn main() {}
