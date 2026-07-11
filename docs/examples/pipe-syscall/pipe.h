#ifndef PIPE_H
#define PIPE_H

#include <stdint.h>

#define PIPE_BUF_SIZE 4096          // fixed-size circular buffer
#define MAX_PIPES      64
#define MAX_FDS        128

// ── Pipe descriptor (in-kernel) ──
typedef struct {
    uint8_t  buf[PIPE_BUF_SIZE];
    uint32_t head;                  // producer index (write)
    uint32_t tail;                  // consumer index (read)
    uint32_t count;                 // bytes currently in buffer
    int      readers;               // number of open read fds
    int      writers;               // number of open write fds
} pipe_t;

// ── File descriptor table (per-process) ──
// ponytail: flat array, no refcounting; slab allocator would be better.
typedef enum { FD_NONE, FD_PIPE_R, FD_PIPE_W } fd_kind_t;

typedef struct {
    fd_kind_t kind;
    int       pipe_id;              // index into pipe_table[]
} fd_entry_t;

// ── Public API ──
int  pipe_syscall(int fd[2]);                        // kernel: create pipe
int  read_syscall(int fd, void *buf, uint32_t len);  // kernel: read from fd
int  write_syscall(int fd, const void *buf, uint32_t len); // kernel: write to fd
int  close_syscall(int fd);                           // kernel: close fd

// Per-process table (one instance per process in real kernel)
extern fd_entry_t fd_table[MAX_FDS];
extern pipe_t pipe_table[MAX_PIPES];

#endif
