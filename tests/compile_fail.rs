#[test]
fn type_boundaries_do_not_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/custom_index_key.rs");
    t.compile_fail("tests/ui/key_fabrication.rs");
    t.compile_fail("tests/ui/key_tags.rs");
    t.compile_fail("tests/ui/leased_bytes_slice.rs");
    t.compile_fail("tests/ui/linked_pool_chain_outlives_pool.rs");
    t.compile_fail("tests/ui/pin_slab_key_tags.rs");
    t.compile_fail("tests/ui/pin_cell_slab_vacant_entry.rs");
    t.compile_fail("tests/ui/pin_slab_take.rs");
    t.compile_fail("tests/ui/region_permission_domain.rs");
    t.compile_fail("tests/ui/zero_generation_limit.rs");
}
