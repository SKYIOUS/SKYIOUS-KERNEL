# ATA PIO driver build (freestanding library).
#
# make          → libata.a
# make test     → userspace smoke-test via QEMU + -nographic (Linux x86_64)
# make clean

CC      ?= gcc
AR      ?= ar
CFLAGS   = -m64 -ffreestanding -nostdlib -fno-stack-protector \
           -mno-red-zone -mno-sse -mno-mmx -O2 -I.
ARFLAGS  = rcs

.PHONY: all test clean

all: libata.a

libata.a: ata.o
	$(AR) $(ARFLAGS) $@ $^

ata.o: ata.c ata.h
	$(CC) $(CFLAGS) -c -o $@ $<

# ── optional QEMU smoke test (Linux cross-linker required) ──
test: test_ata.c ata.c ata.h test.ld
	x86_64-elf-gcc $(CFLAGS) -Wl,-T,test.ld -nostartfiles -nodefaultlibs \
	      -o test_ata.elf test_ata.c ata.c
	x86_64-elf-objcopy -O binary test_ata.elf test_ata.bin
	qemu-system-x86_64 -kernel test_ata.bin -m 32M -smp 1 -no-reboot \
	  -drive if=ide,format=raw,file=test_disk.img -nographic 2>&1 | head -20

clean:
	rm -f *.o *.a *.elf *.bin
