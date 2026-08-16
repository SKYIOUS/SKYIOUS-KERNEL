// ---------------------------------------------------------------------------
// Test Framework
// ---------------------------------------------------------------------------
//
// Simple test framework for no_std kernel testing. Tests are run during
// boot and results are printed via serial output.

pub mod apic_tests;
pub mod ebpf_tests;
pub mod skyfs_tests;
pub mod new_features;
pub mod pata_read_test;
pub mod ext2_fs_tests;
pub mod futex_test;
pub mod vfs_tests;
pub mod memory_tests;
pub mod scheduler_tests;
#[cfg(not(target_arch = "aarch64"))]
pub mod sync_tests;

/// Test function signature
pub type TestFn = fn() -> Result<(), &'static str>;

/// Run all registered tests and print results.
pub fn run_all() {
    crate::serial_write("[TESTS] Starting test suite\n");
    
    let tests: &[TestFn] = &[
        crate::tests::apic_tests::test_msi_alloc,
        crate::tests::apic_tests::test_msi_alloc_range,
        crate::tests::apic_tests::test_apic_mode_detect,
        crate::tests::apic_tests::test_set_tpr,
    ];
    
    let mut passed = 0;
    let mut failed = 0;
    
    for (i, test) in tests.iter().enumerate() {
        crate::serial_write(&alloc::format!("[TESTS] Running test {}...\n", i));
        match test() {
            Ok(_) => {
                passed += 1;
                crate::serial_write(&alloc::format!("[TESTS] test {}: PASS\n", i));
            }
            Err(e) => {
                failed += 1;
                crate::serial_write(&alloc::format!("[TESTS] test {}: FAIL - {}\n", i, e));
            }
        }
    }
    
    crate::serial_write(&alloc::format!(
        "[TESTS] Complete: {} passed, {} failed, {} total\n",
        passed, failed, tests.len()
    ));
}

/// Register every suite into the selftest (TAP) framework.
pub fn register_all() {
    ebpf_tests::register();
    skyfs_tests::register();
    new_features::register_all();
    ext2_fs_tests::register();
    futex_test::register_all();
    vfs_tests::register();
    memory_tests::register();
    scheduler_tests::register();
    #[cfg(not(target_arch = "aarch64"))]
    sync_tests::register();
}
