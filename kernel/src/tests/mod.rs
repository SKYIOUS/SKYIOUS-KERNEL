pub mod ebpf_tests;
pub mod skyfs_tests;
pub mod new_features;

pub fn register_all() {
    ebpf_tests::register();
    skyfs_tests::register();
    new_features::register_all();
}
