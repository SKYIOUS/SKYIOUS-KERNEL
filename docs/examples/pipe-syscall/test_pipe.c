// ── Pipe test: spawns two threads, pipes data between them ──

#include "pipe.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>

static int test_fds[2];

static void *writer_thread(void *arg) {
    (void)arg;
    const char *msg = "Hello from pipe!  The quick brown fox jumps over the lazy dog.";
    int len = (int)strlen(msg) + 1;

    printf("[writer] writing %d bytes...\n", len);
    int w = write_syscall(test_fds[1], msg, len);
    printf("[writer] wrote %d bytes\n", w);

    const char *chunks[] = {"chunk1 ", "chunk2 ", "chunk3!", NULL};
    for (int i = 0; chunks[i]; i++) {
        int n = (int)strlen(chunks[i]);
        write_syscall(test_fds[1], chunks[i], n);
        printf("[writer] chunk %d: %s", i + 1, chunks[i]);
    }
    printf("\n");

    close_syscall(test_fds[1]);
    printf("[writer] closed write end\n");
    return NULL;
}

static void *reader_thread(void *arg) {
    (void)arg;
    char buf[128];
    int total = 0;

    // Read until EOF (writer closed the write end)
    while (1) {
        int r = read_syscall(test_fds[0], buf, sizeof(buf) - 1);
        if (r <= 0) break;
        buf[r] = 0;
        printf("[reader]  read %d bytes: \"%s\"\n", r, buf);
        total += r;
    }
    printf("[reader] EOF after %d total bytes\n", total);

    close_syscall(test_fds[0]);
    return NULL;
}

int main(void) {
    memset(fd_table, 0, sizeof(fd_table));
    memset(pipe_table, 0, sizeof(pipe_table));

    if (pipe_syscall(test_fds)) {
        fprintf(stderr, "pipe() failed\n");
        return 1;
    }
    printf("[main] pipe fds = {%d, %d}\n", test_fds[0], test_fds[1]);

    // Start reader first (will block/spin until data arrives)
    pthread_t r_thr, w_thr;
    pthread_create(&r_thr, NULL, reader_thread, NULL);
    pthread_create(&w_thr, NULL, writer_thread, NULL);

    pthread_join(w_thr, NULL);
    pthread_join(r_thr, NULL);

    printf("[main] readers=%d writers=%d (should be 0 after close)\n",
           pipe_table[0].readers, pipe_table[0].writers);
    printf("[main] done\n");
    return 0;
}
