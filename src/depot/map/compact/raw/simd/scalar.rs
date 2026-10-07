use crate::depot::map::compact;

#[inline]
pub(in crate::depot::map::compact::raw) fn group_masks(bytes: &[u8; 32], tag: u8) -> (u32, u32) {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const LOW: u64 = 0x7f7f_7f7f_7f7f_7f7f;
    const HIGH: u64 = 0x8080_8080_8080_8080;

    // Packs the eight lane flags into consecutive bits.
    #[inline]
    fn pack(word: u64) -> u32 {
        (word.wrapping_mul(0x0002_0408_1020_4081) >> 56) as u32
    }

    // Low seven-bit sums cannot carry into another byte.
    #[inline]
    fn equal(word: u64, byte: u8) -> u64 {
        let x = word ^ (u64::from(byte) * ONES);
        !(x & LOW).wrapping_add(LOW) & !x & HIGH
    }

    let mut vacant = 0u32;
    let mut matched = 0u32;
    for (index, chunk) in bytes.as_chunks::<8>().0.iter().enumerate() {
        let word = u64::from_le_bytes(*chunk);
        vacant |= pack(equal(word, compact::EMPTY)) << (index * 8);
        matched |= pack(equal(word, tag)) << (index * 8);
    }
    (vacant, matched)
}
