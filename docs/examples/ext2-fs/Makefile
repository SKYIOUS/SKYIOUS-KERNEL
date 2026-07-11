# EXT2 read-only driver — library + host test.
#
# make          → libext2.a
# make test     → build + run the Linux userspace test (needs mkfs.ext2)
# make clean

CC      ?= gcc
AR      ?= ar
CFLAGS   = -O2 -Wall -Wextra -I.
ARFLAGS  = rcs

.PHONY: all test clean

all: libext2.a

libext2.a: ext2.o
	$(AR) $(ARFLAGS) $@ $^

ext2.o: ext2.c ext2.h
	$(CC) $(CFLAGS) -c -o $@ $<

# ── host test ──
test: test_ext2.c ext2.c ext2.h create_test_image.sh
	$(CC) $(CFLAGS) -o test_ext2 test_ext2.c ext2.c
	sh create_test_image.sh test.img
	./test_ext2 test.img

clean:
	rm -f *.o *.a test_ext2 test.img
