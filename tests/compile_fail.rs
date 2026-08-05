#[test]
fn type_boundaries_do_not_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/avl_adapter_mismatch.rs");
    t.compile_fail("tests/ui/branded_cell_borrow_mut.rs");
    t.compile_fail("tests/ui/checked_cell_borrow_escape.rs");
    t.compile_fail("tests/ui/custom_index_key.rs");
    t.compile_fail("tests/ui/key_fabrication.rs");
    t.compile_fail("tests/ui/key_tags.rs");
    t.compile_fail("tests/ui/lease_slab_escape.rs");
    t.compile_fail("tests/ui/leased_bytes_slice.rs");
    t.compile_fail("tests/ui/initialized_shared_pool_spare_writer.rs");
    t.compile_fail("tests/ui/owned_capacity_policy_mismatch.rs");
    t.compile_fail("tests/ui/pin_slab_key_tags.rs");
    t.compile_fail("tests/ui/pin_slab_take.rs");
    t.compile_fail("tests/ui/prefix_target_mutation.rs");
    t.compile_fail("tests/ui/region_permission_domain.rs");
    t.compile_fail("tests/ui/return_permit_borrow.rs");
    t.compile_fail("tests/ui/spare_writer_raw_commit_requires_unsafe.rs");
    t.compile_fail("tests/ui/uninitialized_shared_pool_spare.rs");
    t.compile_fail("tests/ui/zero_generation_limit.rs");
}
