# Common Build Issues and Solutions

This page documents common build problems and their solutions.

## Build Failures

### "target not found: x86_64-skyos-unknown"

```bash
rustup target add x86_64-unknown-none --toolchain nightly
```

The custom target spec should be in the project root. If missing, copy it from `config/x86_64-skyos.json`.

### "linker not found: rust-lld"

```bash
rustup component add llvm-tools-preview --toolchain nightly
```

### "failed to run custom build command for cc"

Install a C compiler:
```bash
# Ubuntu/Debian
sudo apt-get install build-essential

# macOS
xcode-select --install
```

## Runtime Issues in QEMU

### "Kernel panics immediately"

- Check that you're using a supported QEMU version
- Try with `-cpu max` flag
- Ensure sufficient memory: `-m 512M`

### "No serial output"

- Verify the `-serial stdio` flag is passed to QEMU
- Check console configuration in the kernel command line

### "Boot hangs after UEFI logo"

- The bootloader may be failing to load the kernel ELF
- Check that `bootimage-skyos.bin` was created correctly
- Try with the `-machine q35` flag

## Common Mistakes

- **Forgetting to build with release mode**: Debug builds are significantly slower
- **Missing Rust components**: Always run `rustup component add` for new Rust installations
- **Incorrect Python version**: Build scripts require Python 3.8+
- **Outdated QEMU**: Some CPU features require QEMU 6.0+
