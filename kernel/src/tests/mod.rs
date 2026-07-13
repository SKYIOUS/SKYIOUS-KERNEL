pub mod ebpf_tests;
pub mod skyfs_tests;
pub mod new_features;
pub mod pata_read_test;
pub mod ext2_fs_tests;
pub mod futex_test;
pub mod vfs_tests;
pub mod memory_tests;
pub mod scheduler_tests;

pub fn register_all() {
    ebpf_tests::register();
    skyfs_tests::register();
    new_features::register_all();
    ext2_fs_tests::register();
    futex_test::register_all();
    vfs_tests::register();
    memory_tests::register();
    scheduler_tests::register();
}
