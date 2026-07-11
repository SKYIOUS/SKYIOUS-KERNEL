// ── Pipe syscall: circular buffer, blocking read/write, close ──

#include "pipe.h"
#include <stddef.h>
#include <string.h>

fd_entry_t fd_table[MAX_FDS];
pipe_t pipe_table[MAX_PIPES];
static int pipe_count = 0;

// ── Interrupt save/restore ──
// User-space test compiles with -DNO_CLI (cli/sti are ring-0 only).

#ifndef NO_CLI
static inline void irq_save(uint64_t *flags) {
    __asm__ __volatile__("pushfq; pop %0; cli" : "=r"(*flags) : : "cc");
}
static inline void irq_restore(uint64_t flags) {
    if (flags & 0x200) __asm__ __volatile__("sti");
}
#else
static inline void irq_save(uint64_t *flags) { (void)flags; }
static inline void irq_restore(uint64_t flags) { (void)flags; }
#endif

// ponytail: spin-wait; real kernel uses wait queue + scheduler yield.
static void pipe_wait_read(pipe_t *p) {
    while (p->count == 0 && p->writers > 0)
        __asm__ __volatile__("pause");
}

static void pipe_wait_write(pipe_t *p) {
    while (p->count >= PIPE_BUF_SIZE && p->readers > 0)
        __asm__ __volatile__("pause");
}

static pipe_t *fd_to_pipe(int fd, int require_writer) {
    if (fd < 0 || fd >= MAX_FDS) return NULL;
    fd_entry_t *e = &fd_table[fd];
    if (e->kind == FD_NONE) return NULL;
    fd_kind_t want = require_writer ? FD_PIPE_W : FD_PIPE_R;
    if (e->kind != want) return NULL;
    if (e->pipe_id < 0 || e->pipe_id >= pipe_count) return NULL;
    return &pipe_table[e->pipe_id];
}

int pipe_syscall(int fd[2]) {
    uint64_t flags;
    irq_save(&flags);

    if (pipe_count >= MAX_PIPES) { irq_restore(flags); return -1; }

    int r_fd = -1, w_fd = -1;
    for (int i = 0; i < MAX_FDS; i++) {
        if (fd_table[i].kind == FD_NONE) {
            if (r_fd < 0) r_fd = i;
            else if (w_fd < 0) { w_fd = i; break; }
        }
    }
    if (w_fd < 0) { irq_restore(flags); return -1; }

    pipe_t *p = &pipe_table[pipe_count++];
    p->head = p->tail = p->count = 0;
    p->readers = 1;
    p->writers = 1;

    fd_table[r_fd].kind    = FD_PIPE_R;
    fd_table[r_fd].pipe_id = pipe_count - 1;
    fd_table[w_fd].kind    = FD_PIPE_W;
    fd_table[w_fd].pipe_id = pipe_count - 1;

    fd[0] = r_fd;
    fd[1] = w_fd;

    irq_restore(flags);
    return 0;
}

int read_syscall(int fd, void *buf, uint32_t len) {
    uint64_t flags;
    irq_save(&flags);

    pipe_t *p = fd_to_pipe(fd, 0);
    if (!p) { irq_restore(flags); return -1; }

    if (p->count == 0 && p->writers == 0) { irq_restore(flags); return 0; }
    pipe_wait_read(p);

    uint32_t total = 0;
    uint8_t *dst = (uint8_t *)buf;

    while (len > 0 && p->count > 0) {
        dst[total++] = p->buf[p->tail];
        p->tail = (p->tail + 1) & (PIPE_BUF_SIZE - 1);
        p->count--;
        len--;
    }

    irq_restore(flags);
    return (int)total;
}

int write_syscall(int fd, const void *buf, uint32_t len) {
    uint64_t flags;
    irq_save(&flags);

    pipe_t *p = fd_to_pipe(fd, 1);
    if (!p) { irq_restore(flags); return -1; }

    uint32_t total = 0;
    const uint8_t *src = (const uint8_t *)buf;

    while (total < len) {
        pipe_wait_write(p);
        if (p->readers == 0) { irq_restore(flags); return -1; }

        uint32_t space = PIPE_BUF_SIZE - p->count;
        uint32_t chunk = len - total;
        if (chunk > space) chunk = space;

        for (uint32_t i = 0; i < chunk; i++) {
            p->buf[p->head] = src[total++];
            p->head = (p->head + 1) & (PIPE_BUF_SIZE - 1);
            p->count++;
        }
    }

    irq_restore(flags);
    return (int)total;
}

int close_syscall(int fd) {
    uint64_t flags;
    irq_save(&flags);

    if (fd < 0 || fd >= MAX_FDS) { irq_restore(flags); return -1; }
    fd_entry_t *e = &fd_table[fd];
    if (e->kind == FD_NONE) { irq_restore(flags); return -1; }

    pipe_t *p = &pipe_table[e->pipe_id];
    if (e->kind == FD_PIPE_R) p->readers--;
    if (e->kind == FD_PIPE_W) p->writers--;

    e->kind = FD_NONE;
    e->pipe_id = -1;

    irq_restore(flags);
    return 0;
}
