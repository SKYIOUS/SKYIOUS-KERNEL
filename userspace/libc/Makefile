# Makefile for vDSO libc test
# 
# Requires musl-gcc (or gcc with -static and proper sysroot)
# 
# Usage:
#   make                    # Build with musl-gcc
#   make test               # Build and run in QEMU (requires built kernel image)
#   make clean              # Clean build artifacts

CC = musl-gcc
CFLAGS = -static -Os -Wall -Wextra -D VDSO_LIBC_TEST
TARGET = vdso_libc_test
SRC = vdso_libc.c vdso_libc_test.c

all: $(TARGET)

$(TARGET): $(SRC)
	$(CC) $(CFLAGS) -o $@ $^

test: $(TARGET)
	@echo "To test in QEMU:"
	@echo "  1. Copy $(TARGET) to ramdisk/initrd"
	@echo "  2. Boot kernel with initrd containing $(TARGET) as /bin/init"
	@echo "  3. Or run manually in QEMU shell: ./$(TARGET)"

clean:
	rm -f $(TARGET)

.PHONY: all test clean