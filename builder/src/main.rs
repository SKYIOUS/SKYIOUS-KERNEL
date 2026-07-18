use std::path::Path;

fn main() -> anyhow::Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let arch = std::env::var("VAHI_ARCH").unwrap_or_else(|_| "x86_64".into());

    let (target_triple, out_dir_name) = match arch.as_str() {
        "aarch64" => ("aarch64-unknown-none", "aarch64-vahi"),
        _         => ("x86_64-unknown-none",  "x86_64-vahi"),
    };

    let kernel_path = root_dir.join(format!("kernel/target/{}/{}/vahi_kernel", target_triple, profile));
    let initrd_path = root_dir.join("kernel/initrd.tar");

    let out_dir = root_dir.join(format!("target/{}/{}", out_dir_name, profile));
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)?;
    }

    let uefi_path = out_dir.join("bootimage-vahi_kernel.bin");

    println!("Building UEFI bootimage [{}] arch={}: {:?}", profile, arch, uefi_path);
    if !kernel_path.exists() {
        anyhow::bail!("Kernel ELF not found at {:?}. Build the kernel for {} first.", kernel_path, arch);
    }
    if initrd_path.exists() {
        println!("  initrd: {:?}", initrd_path);
    } else {
        println!("  WARNING: no initrd at {:?}", initrd_path);
    }

    let mut boot = bootloader::UefiBoot::new(&kernel_path);
    if initrd_path.exists() {
        boot.set_ramdisk(&initrd_path);
    }
    boot.create_disk_image(&uefi_path)?;

    println!("SUCCESS: Created UEFI bootimage at {:?}", uefi_path);
    Ok(())
}
