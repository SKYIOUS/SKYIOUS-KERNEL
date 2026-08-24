# Vahi Kernel — Top-level Makefile
#
# Targets:
#   make boot       Build kernel + create bootable disk image
#   make run         Build + boot in QEMU (serial output)
#   make run-debug   Build + boot with GDB stub
#   make kernel      Build kernel only (release)
#   make kernel-debug Build kernel only (debug)
#   make image       Create disk image only (kernel must be built)
#   make clean       Remove build artifacts
#   make help        Show this help

KERNEL_DIR  := kernel
BUILDER_DIR := builder
PROFILE     := release
CARGO_FLAGS := --release --target x86_64-unknown-none
IMAGE       := bootimage-vahi_kernel.bin

.PHONY: boot run run-debug kernel kernel-debug image clean help

# Default target
all: boot

# Build kernel + disk image
boot: kernel image
	@echo ""
	@echo "=== Ready ==="
	@echo "Image: $(IMAGE)"
	@echo "Run:   make run"

# Build + run in QEMU
run: boot
	@scripts/run_qemu.sh

# Build + run with GDB stub
run-debug: boot
	@scripts/run_qemu.sh --debug

# Build kernel only (release)
kernel:
	cargo build $(CARGO_FLAGS) -p vahi_kernel

# Build kernel only (debug)
kernel-debug:
	cargo build --target x86_64-unknown-none -p vahi_kernel

# Create disk image only
image:
	python3 $(BUILDER_DIR)/build_limine_image.py \
		--kernel $(KERNEL_DIR)/target/x86_64-unknown-none/$(PROFILE)/vahi_kernel \
		--output $(IMAGE)

# Clean build artifacts
clean:
	cd $(KERNEL_DIR) && cargo clean
	rm -f $(IMAGE)

# Show help
help:
	@echo "Vahi Kernel Build System"
	@echo ""
	@echo "  make boot        Build kernel + create bootable image"
	@echo "  make run         Build + boot in QEMU"
	@echo "  make run-debug   Build + boot with GDB stub"
	@echo "  make kernel      Build kernel only (release)"
	@echo "  make kernel-debug Build kernel only (debug)"
	@echo "  make image       Create disk image only"
	@echo "  make clean       Remove build artifacts"
