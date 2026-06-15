pub mod ebpf_tests;
pub mod skyfs_tests;

pub fn register_all() {
    ebpf_tests::register();
    skyfs_tests::register();
}
