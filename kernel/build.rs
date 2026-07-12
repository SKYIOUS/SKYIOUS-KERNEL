fn main() {
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=aarch64-linker.ld");

    // The linker script is set per-target in .cargo/config.toml, so no need to
    // add it here unless we want to override. This build.rs currently just
    // triggers rebuilds when linker scripts change.
}
