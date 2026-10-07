use std::arch::x86_64::*;

use crate::depot::map::compact;

#[inline]
pub(in crate::depot::map::compact::raw) fn group_masks(bytes: &[u8; 32], tag: u8) -> (u32, u32) {
    let mut vacant = 0;
    let mut matched = 0;
    // SAFETY: the exact chunks admit both unaligned loads; this module requires SSE2.
    unsafe {
        let empty = _mm_set1_epi8(compact::EMPTY as i8);
        let tag = _mm_set1_epi8(tag as i8);
        for (index, chunk) in bytes.as_chunks::<16>().0.iter().enumerate() {
            let group = _mm_loadu_si128(chunk.as_ptr().cast());
            vacant |= (_mm_movemask_epi8(_mm_cmpeq_epi8(group, empty)) as u32) << (index * 16);
            matched |= (_mm_movemask_epi8(_mm_cmpeq_epi8(group, tag)) as u32) << (index * 16);
        }
    }
    (vacant, matched)
}
