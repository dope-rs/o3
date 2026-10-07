use std::{
    env,
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn brands_and_indices() {
    let exe = env::current_exe().expect("test executable");
    let profile = exe.parent().expect("deps").parent().expect("profile");
    let name = profile
        .file_name()
        .expect("profile name")
        .to_str()
        .expect("profile UTF-8");
    let status = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["build", "--offline", "--locked", "--lib", "--profile"])
        .arg(if name == "debug" { "dev" } else { name })
        .arg("--target-dir")
        .arg(profile.parent().expect("target directory"))
        .status()
        .expect("build original library");
    assert!(status.success());

    for (source, expected) in [
        (include_str!("common/compile_contracts.rs"), ""),
        (include_str!("common/compile_retain.rs"), "error[E0200]:"),
        (include_str!("common/compile_provided.rs"), "error[E0200]:"),
        (
            include_str!("common/compile_time_average.rs"),
            "error[E0080]: evaluation panicked:",
        ),
        (
            include_str!("common/compile_escape.rs"),
            "error[E0521]: borrowed data escapes outside of closure",
        ),
        (
            include_str!("common/compile_owner.rs"),
            "error[E0521]: borrowed data escapes outside of closure",
        ),
        (
            include_str!("common/compile_mem_slot.rs"),
            "error[E0521]: borrowed data escapes outside of closure\n\
             error[E0505]: cannot move out of `slot` because it is borrowed",
        ),
        (
            include_str!("common/compile_array_unfilled.rs"),
            "error[E0308]: mismatched types",
        ),
        (
            include_str!("common/compile_array_receipt.rs"),
            "error: lifetime may not live long enough",
        ),
        (
            include_str!("common/compile_array_variance.rs"),
            "error[E0597]: `value` does not live long enough",
        ),
        (
            include_str!("common/compile_prefix_size.rs"),
            "error[E0080]: evaluation panicked: size",
        ),
        (
            include_str!("common/compile_prefix_align.rs"),
            "error[E0080]: evaluation panicked: align",
        ),
        (
            include_str!("common/compile_prefix_capacity.rs"),
            "error[E0080]: evaluation panicked: assertion failed: N <= u8::MAX as usize + 1",
        ),
        (
            include_str!("common/compile_prefix_owner.rs"),
            "error[E0521]: borrowed data escapes outside of closure",
        ),
        (
            include_str!("common/compile_prefix_escape.rs"),
            "error: lifetime may not live long enough",
        ),
        (
            include_str!("common/compile_prefix_borrow.rs"),
            "error: lifetime may not live long enough",
        ),
        (
            include_str!("common/compile_planes_owner.rs"),
            "error[E0521]: borrowed data escapes outside of closure",
        ),
        (
            include_str!("common/compile_planes_borrow.rs"),
            "error[E0499]: cannot borrow `resident` as mutable more than once at a time\n\
             error[E0505]: cannot move out of `terminal` because it is borrowed",
        ),
        (
            include_str!("common/compile_fixed_owner.rs"),
            "error[E0499]: cannot borrow `*owner` as mutable more than once at a time",
        ),
        (
            include_str!("common/compile_fixed_capacity.rs"),
            "error[E0308]: mismatched types",
        ),
        (
            include_str!("common/compile_resident_owner.rs"),
            "error[E0521]: borrowed data escapes outside of closure",
        ),
        (
            include_str!("common/compile_fifo_owner.rs"),
            "error[E0521]: borrowed data escapes outside of closure",
        ),
        (
            include_str!("common/compile_resident_phase.rs"),
            "error[E0382]: borrow of moved value: `scope`\n\
             error[E0382]: use of moved value: `fresh`",
        ),
        (
            include_str!("common/compile_window_borrow.rs"),
            "error[E0505]: cannot move out of `window` because it is borrowed",
        ),
        (
            include_str!("common/compile_scope_select.rs"),
            "error: lifetime may not live long enough",
        ),
        (
            include_str!("common/compile_consume_end.rs"),
            "error: lifetime may not live long enough",
        ),
        (
            include_str!("common/compile_scope_source.rs"),
            "error[E0599]: the method `view` exists\n\
             error[E0599]: the method `borrow` exists\n\
             error[E0599]: the method `borrow` exists",
        ),
        (
            include_str!("common/compile_plane.rs"),
            "error[E0080]: evaluation panicked: plane",
        ),
        (
            include_str!("common/compile_axis.rs"),
            "error[E0308]: mismatched types",
        ),
        (
            include_str!("common/compile_endpoint.rs"),
            "error[E0599]: no method named `into_store` found for struct",
        ),
        (
            include_str!("common/compile_round_head_clone.rs"),
            "error[E0599]: the method `clone` exists",
        ),
        (
            include_str!("common/compile_round_head_reuse.rs"),
            "error[E0382]: use of moved value: `head`",
        ),
        (
            include_str!("common/compile_fair_mutation.rs"),
            "error[E0624]: method `lane_mut` is private",
        ),
        (
            include_str!("common/compile_fair_owner.rs"),
            "error[E0616]: field `inner` of struct",
        ),
        (
            include_str!("common/compile_ring_mutation.rs"),
            "error[E0599]: no method named `slices_mut` found",
        ),
        (
            include_str!("common/compile_lanes_idle.rs"),
            "error[E0599]:",
        ),
        (
            include_str!("common/compile_lanes_phase.rs"),
            "error[E0277]:",
        ),
        (
            include_str!("common/compile_lanes_running.rs"),
            "error[E0603]:",
        ),
        (
            include_str!("common/compile_resident_lease.rs"),
            "error[E0277]: the trait bound",
        ),
        (
            include_str!("common/compile_indexed_key.rs"),
            "error[E0624]: method `key` is private",
        ),
        (
            include_str!("common/compile_slab_key.rs"),
            "error[E0277]: the trait bound `Custom:\n\
             Numeric` is not satisfied\n\
             error[E0599]: no method named `key` found",
        ),
        (
            include_str!("common/compile_slab_generation.rs"),
            "error[E0080]: evaluation panicked: generation limit must be nonzero",
        ),
        (
            include_str!("common/compile_slab_generation_empty.rs"),
            "error[E0080]: evaluation panicked: generation limit must be nonzero",
        ),
        (
            include_str!("common/compile_vacancy.rs"),
            "error[E0080]: evaluation panicked: assertion failed: I < N",
        ),
        (
            include_str!("common/compile_widen.rs"),
            "error[E0080]: evaluation panicked: widen",
        ),
        (
            include_str!("common/compile_widen_lower.rs"),
            "error[E0080]: evaluation panicked: widen",
        ),
        (
            include_str!("common/compile_pool_width.rs"),
            "error[E0080]: evaluation panicked: width",
        ),
        (
            include_str!("common/compile_pool_layout.rs"),
            "error[E0080]: evaluation panicked: layout",
        ),
    ] {
        let mut compiler = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .current_dir(profile)
            .args([
                "-Znext-solver=globally",
                "--edition=2024",
                "--crate-name=o3_compile_contract",
                "--crate-type=bin",
                "-Cpanic=abort",
                "-Copt-level=3",
                "--emit=llvm-ir=-",
                "--color=never",
                "--extern",
            ])
            .arg(format!("o3={}", profile.join("libo3.rlib").display()))
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("compiler");
        compiler
            .stdin
            .take()
            .expect("compiler stdin")
            .write_all(source.as_bytes())
            .expect("fixture input");
        let output = compiler.wait_with_output().expect("compiler output");
        let diag = String::from_utf8(output.stderr).expect("diagnostic UTF-8");
        assert!(
            output.status.success() == expected.is_empty()
                && expected
                    .lines()
                    .try_fold(diag.as_str(), |diag, expected| {
                        diag.split_once(expected).map(|(_, rest)| rest)
                    })
                    .is_some(),
            "{source}\n{diag}"
        );
    }
}
