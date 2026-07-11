CC      ?= gcc
AS      ?= as
OBJCOPY ?= objcopy
PYTHON  ?= python3

STAGE1_SRC := stage1/stage1.s
STAGE2_SRC := stage2/stage2.c
LD_SCRIPT  := stage2/linker.ld

STAGE1_BIN := stage1.bin
STAGE2_PE  := stage2.pe
STAGE2_BIN := stage2_flat.bin
IMAGE_BIN  := image.bin
ISO_DIR    := iso

# Path to kernel.bin from the main kernel build
KERNEL_BIN ?= ../target/x86_64-vahi/debug/vahi_kernel.bin

.PHONY: all clean image iso qemu

all: image

# ── stage1 ──
$(STAGE1_BIN): $(STAGE1_SRC)
	$(AS) --32 -o $*.o $< && $(OBJCOPY) -O binary $*.o $@

# ── stage2: C → PE → flat binary ──
# Mingw linker only produces PE output, so we extract raw sections.
$(STAGE2_PE): $(STAGE2_SRC) $(LD_SCRIPT)
	$(CC) -m16 -march=i386 -ffreestanding -nostdlib -Os \
	      -fno-pic -fno-stack-protector -fno-builtin \
	      -nostartfiles -nodefaultlibs \
	      -Wl,-T,$(LD_SCRIPT),--image-base,0,--section-alignment,512,--file-alignment,512 \
	      -o $@ $<

$(STAGE2_BIN): bin2flat.py $(STAGE2_PE)
	$(PYTHON) bin2flat.py --pe $(STAGE2_PE) --output $@

# ── disk image ──
image: $(STAGE1_BIN) $(STAGE2_BIN)
	$(PYTHON) build_image.py \
	    --stage1 $(STAGE1_BIN) \
	    --stage2 $(STAGE2_BIN) \
	    --kernel $(KERNEL_BIN) \
	    --output $(IMAGE_BIN)

# ── ISO (El Torito) ──
iso: image
	rm -rf $(ISO_DIR)
	mkdir -p $(ISO_DIR)/boot
	cp $(IMAGE_BIN) $(ISO_DIR)/boot/
	xorriso -as mkisofs \
	    -b boot/image.bin \
	    -no-emul-boot \
	    -boot-load-size 4 \
	    -o skyos-bootloader.iso \
	    $(ISO_DIR)

# ── QEMU ──
qemu: image
	qemu-system-x86_64 -drive if=ide,format=raw,file=$(IMAGE_BIN) -m 64M -smp 1

clean:
	rm -f $(STAGE1_BIN) $(STAGE2_PE) $(STAGE2_BIN) $(IMAGE_BIN)
	rm -rf $(ISO_DIR)
