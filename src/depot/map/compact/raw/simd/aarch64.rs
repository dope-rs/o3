use std::arch::aarch64::*;

use crate::depot::map::compact;

#[inline]
pub(in crate::depot::map::compact::raw) fn group_masks(bytes: &[u8; 32], tag: u8) -> (u32, u32) {
    #[inline]
    fn pack(low: uint8x16_t, high: uint8x16_t) -> u32 {
        // SAFETY: this module requires NEON; each weighted eight-byte sum is <=255.
        unsafe {
            let weights =
                vld1q_u8([1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128].as_ptr());
            let pairs = vpaddq_u8(vandq_u8(low, weights), vandq_u8(high, weights));
            let quads = vpaddq_u8(pairs, pairs);
            let octets = vpaddq_u8(quads, quads);
            // NEON lane extraction orders each packed pair in native byte order.
            let low = u16::from_ne_bytes([vgetq_lane_u8::<0>(octets), vgetq_lane_u8::<1>(octets)]);
            let high = u16::from_ne_bytes([vgetq_lane_u8::<2>(octets), vgetq_lane_u8::<3>(octets)]);
            u32::from(low) | (u32::from(high) << 16)
        }
    }

    // SAFETY: the array admits both unaligned loads, and this module requires NEON.
    unsafe {
        let low = vld1q_u8(bytes.as_ptr());
        let high = vld1q_u8(bytes.as_ptr().add(16));
        let empty = vdupq_n_u8(compact::EMPTY);
        let tag = vdupq_n_u8(tag);
        (
            pack(vceqq_u8(low, empty), vceqq_u8(high, empty)),
            pack(vceqq_u8(low, tag), vceqq_u8(high, tag)),
        )
    }
}
