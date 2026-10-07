cfg_select! {
    all(target_arch = "aarch64", target_feature = "neon") => {
        mod aarch64;
        pub(super) use aarch64::group_masks;
    }
    all(target_arch = "x86_64", target_feature = "sse2") => {
        mod x86;
        pub(super) use x86::group_masks;
    }
    _ => {
        mod scalar;
        pub(super) use scalar::group_masks;
    }
}
