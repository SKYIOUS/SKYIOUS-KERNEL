use std::path::Path;

fn main() -> anyhow::Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    
    // Support both debug and release profiles
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let kernel_path = root_dir.join(format!("kernel/target/x86_64-unknown-none/{}/vahi_kernel", profile));
    
    // Locate initrd — sibling SkyOS repo, or fallback to legacy SkyOS/ in this repo
    let initrd_path = root_dir.join("../../SkyOS/initrd.tar");
    let initrd_path = if initrd_path.exists() {
        initrd_path
    } else {
        root_dir.join("SkyOS/initrd.tar")
    };
    
    let out_dir = root_dir.join(format!("target/x86_64-vahi/{}", profile));
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)?;
    }

    let uefi_path = out_dir.join("bootimage-vahi_kernel.bin");

    println!("Building UEFI bootimage [{}]: {:?}", profile, uefi_path);
    if initrd_path.exists() {
        println!("  initrd: {:?}", initrd_path);
    } else {
        println!("  WARNING: no initrd found at {:?} or {:?}", initrd_path, root_dir.join("SkyOS/initrd.tar"));
    }
    
    let mut boot = bootloader::UefiBoot::new(&kernel_path);
    if initrd_path.exists() {
        boot.set_ramdisk(&initrd_path);
    }
    boot.create_disk_image(&uefi_path)?;

    println!("SUCCESS: Created UEFI bootimage at {:?}", uefi_path);
    
    Ok(())
}
