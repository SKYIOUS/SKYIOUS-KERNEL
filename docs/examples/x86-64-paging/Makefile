# Build a flat binary with 4-level paging + demand paging demo.
# The linker script places everything at 0x100000 for protected-mode entry.
#
# Targets:
#   make          → kernel.bin (flat binary, 1 MiB entry point)
#   make qemu     → boot it via QEMU (multiboot or direct -kernel)
#   make clean

CC      ?= gcc
AS      ?= gcc
LD      ?= gcc
OBJCOPY ?= objcopy

CFLAGS   = -m64 -ffreestanding -nostdlib -fno-stack-protector \
           -mno-red-zone -mno-sse -mno-mmx -O2 -I.
ASFLAGS  = -m64 -ffreestanding -c
LDFLAGS  = -m64 -ffreestanding -nostdlib -static \
           -Wl,-T,linker.ld \
           -Wl,--build-id=none,--oformat,binary

OBJS = startup.o paging.o paging_fault_stub.o

.PHONY: all qemu clean

all: kernel.bin

kernel.bin: $(OBJS)
	$(LD) $(LDFLAGS) -o $@ $^

startup.o: startup.S
	$(AS) $(ASFLAGS) -o $@ $<

paging.o: paging.c paging.h
	$(CC) $(CFLAGS) -c -o $@ $<

paging_fault_stub.o: paging_fault_stub.S
	$(AS) $(ASFLAGS) -o $@ $<

qemu: kernel.bin
	qemu-system-x86_64 -kernel kernel.bin -m 128M -smp 1 -no-reboot -nographic

clean:
	rm -f *.o *.bin
