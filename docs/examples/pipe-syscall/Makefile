CC      ?= gcc
AR      ?= ar
CFLAGS   = -O2 -Wall -Wextra -I.

.PHONY: all test clean

all: libpipe.a

libpipe.a: pipe.o
	$(AR) rcs $@ $^

pipe.o: pipe.c pipe.h
	$(CC) $(CFLAGS) -c -o $@ $<

test: test_pipe.c pipe.c pipe.h
	$(CC) $(CFLAGS) -pthread -o test_pipe test_pipe.c pipe.c -DNO_CLI
	./test_pipe

clean:
	rm -f *.o *.a test_pipe
