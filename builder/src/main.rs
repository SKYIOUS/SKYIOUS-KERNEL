use std::path::Path;

fn main() -> anyhow::Result<()> {
    // The kernel binary is located in kernel/target/x86_64-unknown-none/debug/vahi_kernel
    // We run this from the root directory.
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let kernel_path = root_dir.join("kernel/target/x86_64-unknown-none/debug/vahi_kernel");
    
    // Path for SkyOS/run.ps1 compatibility (now Vahi)
    let out_dir = root_dir.join("target/x86_64-vahi/debug");
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)?;
    }

    let uefi_path = out_dir.join("bootimage-vahi_kernel.bin");

    println!("Building UEFI bootimage for Vahi compatibility: {:?}", uefi_path);
    bootloader::UefiBoot::new(&kernel_path)
        .create_disk_image(&uefi_path)?;

    println!("SUCCESS: Created UEFI bootimage at {:?}", uefi_path);
    
    Ok(())
}
