// ---------------------------------------------------------------------------
// Test Framework
// ---------------------------------------------------------------------------
//
// Simple test framework for no_std kernel testing. Tests are run during
// boot and results are printed via serial output.

pub mod apic_tests;
pub mod benchmarks;
pub mod contract_tests;
pub mod ebpf_tests;
pub mod ext2_fs_tests;
pub mod futex_test;
pub mod fuzzer;
pub mod harness_tests;
pub mod init;
pub mod memory_tests;
pub mod new_features;
pub mod panic_path_tests;
pub mod pata_read_test;
pub mod process_lifecycle_tests;
pub mod scheduler_tests;
pub mod security_tests;
pub mod skyfs_tests;
pub mod stress;
#[cfg(not(target_arch = "aarch64"))]
pub mod sync_tests;
pub mod vfs_tests;

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
    security_tests::register();
    stress::register();
    fuzzer::register();
    // T-00 harness validation demos. The registered variant is selected at
    // build time via VAHI_SELFTEST_MODE; the default registers only the
    // deterministic-pass test, so normal builds are unaffected.
    #[cfg(feature = "self_test")]
    harness_tests::register();
    benchmarks::register();
    contract_tests::register();
    panic_path_tests::register();
    #[cfg(not(target_arch = "aarch64"))]
    sync_tests::register();
    process_lifecycle_tests::register();
}
