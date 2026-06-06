import subprocess
import os
import shutil
import sys

def run_command(command, cwd=None, env=None):
    print(f"Running: {' '.join(command)}")
    full_env = os.environ.copy()
    if env:
        full_env.update(env)
    result = subprocess.run(command, cwd=cwd, shell=True, env=full_env)
    if result.returncode != 0:
        print(f"Error: Command failed with return code {result.returncode}")
        sys.exit(1)

def main():
    root_dir = os.path.dirname(os.path.abspath(__file__))
    kernel_dir = os.path.join(root_dir, "kernel")
    
    print("--- Vahi Kernel Build System ---")
    
    # 1. Clean up old build artifacts if any
    print("Cleaning old images...")
    for ext in ["_uefi.img", ".vdi"]:
        path = os.path.join(root_dir, f"vahi{ext}")
        if os.path.exists(path):
            os.remove(path)
    
    target_uefi = os.path.join(root_dir, "target", "vahi-uefi.img")
    if os.path.exists(target_uefi):
        os.remove(target_uefi)

    # 2. Build the kernel and create bootimage using the new builder
    print("Building kernel...")
    run_command(["cargo", "+nightly", "build"], cwd=kernel_dir)
    
    print("Running image builder...")
    run_command(["cargo", "+nightly", "run", "--manifest-path", "builder/Cargo.toml"], cwd=root_dir, env={"RUST_BACKTRACE": "1"})
    
    # 3. Locate the output file
    uefi_path = os.path.join(root_dir, "target", "x86_64-vahi", "debug", "bootimage-vahi_kernel.bin")
    
    if not os.path.exists(uefi_path):
        print(f"Error: Could not find UEFI image at {uefi_path}")
        sys.exit(1)
    
    # 4. Copy UEFI to root
    output_uefi = os.path.join(root_dir, "vahi_uefi.img")
    shutil.copy2(uefi_path, output_uefi)
    print(f"SUCCESS: Created UEFI disk image at {output_uefi}")
    
    # 5. VirtualBox VDI from UEFI image
    output_vdi = os.path.join(root_dir, "vahi.vdi")
    print(f"Converting {output_uefi} to {output_vdi}...")
    if os.path.exists(output_vdi):
        try:
            os.remove(output_vdi)
        except Exception as e:
            print(f"Warning: Could not remove old VDI (is VirtualBox running?): {e}")
    
    vbox_path = r"C:\Program Files\Oracle\VirtualBox\VBoxManage"
    try:
        run_command([vbox_path, "convertfromraw", output_uefi, output_vdi, "--format", "VDI"])
        print(f"SUCCESS: Created VirtualBox disk at {output_vdi}")
        
        # Resize to 64MB to satisfy some picky firmwares
        print("Resizing VDI to 64MB...")
        # Use subprocess directly to avoid run_command's exit(1)
        subprocess.run([vbox_path, "modifymedium", "disk", output_vdi, "--resize", "64"])
        print("Resize attempt finished.")
    except Exception as e:
        print("Warning: VBoxManage conversion failed.")
        print(f"Error detail: {e}")

    print("\nTo run with QEMU (UEFI):")
    print(f'  qemu-system-x86_64 -bios "OVMF.fd" -drive format=raw,file="{output_uefi}" -m 512M -smp 2')
    print("  (Download OVMF.fd from https://github.com/clearlinux/common/raw/master/OVMF.fd if missing)")
    print("\nFor VirtualBox: Use vahi.vdi with EFI enabled in System > Motherboard settings.")

if __name__ == "__main__":
    main()
