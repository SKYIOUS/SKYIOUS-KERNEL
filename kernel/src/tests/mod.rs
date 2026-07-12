pub mod ebpf_tests;
pub mod skyfs_tests;
pub mod new_features;
pub mod pata_read_test;
pub mod ext2_fs_tests;
pub mod futex_test;

pub fn register_all() {
    ebpf_tests::register();
    skyfs_tests::register();
    new_features::register_all();
    ext2_fs_tests::register();
    futex_test::register_all();
}
