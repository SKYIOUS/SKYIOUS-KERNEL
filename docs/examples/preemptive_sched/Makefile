# Builds a static library for the preemptive scheduler.
# Link into your kernel along with your timer IRQ handler.
#
# Usage:
#   make          → libsched.a
#   make test     → userspace smoke test (requires Linux x86_64)
#   make clean

CC      ?= gcc
AS      ?= gcc
AR      ?= ar
CFLAGS   = -m64 -ffreestanding -nostdlib -fno-stack-protector \
           -mno-red-zone -mno-sse -mno-mmx -O2 -I.
ASFLAGS  = -m64 -ffreestanding -c
ARFLAGS  = rcs

OBJS = sched.o switch.o

.PHONY: all test clean

all: libsched.a

libsched.a: $(OBJS)
	$(AR) $(ARFLAGS) $@ $^

sched.o: sched.c sched.h
	$(CC) $(CFLAGS) -c -o $@ $<

switch.o: switch.S
	$(AS) $(ASFLAGS) -o $@ $<

# ── optional userspace test ──
# Tests enqueue/dequeue + schedule logic without real interrupt hardware.
# Compiles with the host libc for printf etc.
test: test_sched.c sched.c switch.S sched.h
	$(CC) -m64 -g -O0 -o $@ test_sched.c sched.c switch.S -lpthread
	./test

clean:
	rm -f *.o *.a test
