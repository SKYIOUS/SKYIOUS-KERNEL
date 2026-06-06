fn main() {
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=../SkyOS/initrd.tar");
    let initrd_path = std::path::Path::new("../SkyOS/initrd.tar");
    if initrd_path.exists() {
        let data = std::fs::read(initrd_path).unwrap_or_default();
        // Emit hash of the initrd as an env var so cargo recompiles the kernel
        // when initrd content changes.
        let hash = simple_hash(&data);
        println!("cargo:rustc-env=INITRD_HASH={}", hash);
    }
}

fn simple_hash(data: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}
