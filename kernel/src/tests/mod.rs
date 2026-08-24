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
pub mod stress;
pub mod fuzzer;
pub mod benchmarks;
#[cfg(not(target_arch = "aarch64"))]
pub mod sync_tests;

/// Test function signature
pub type TestFn = fn() -> Result<(), &'static str>;

/// Run all registered tests and print results.
/// APIC tests are now registered via the selftest framework (register_all).
pub fn run_all() {
    // No-op: all tests run through selftest::run_all() after register_all().
    // This function exists for backward compatibility with main.rs call sites.
}

/// Register every suite into the selftest (TAP) framework.
pub fn register_all() {
    apic_tests::register();
    ebpf_tests::register();
    skyfs_tests::register();
    new_features::register_all();
    ext2_fs_tests::register();
    futex_test::register_all();
    vfs_tests::register();
    memory_tests::register();
    scheduler_tests::register();
    stress::register();
    fuzzer::register();
    benchmarks::register();
    #[cfg(not(target_arch = "aarch64"))]
    sync_tests::register();
}
