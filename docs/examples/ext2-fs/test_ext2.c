// ── EXT2 test: mount a 16 MB image, list root, read a file ──
// Build:  gcc -O2 -o test_ext2 test_ext2.c ext2.c
// Run:    ./create_test_image.sh; ./test_ext2 test.img
// ponytail: uses host POSIX I/O as the block-device backend.

#include "ext2.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>

static int img_fd = -1;

int host_block_read(uint32_t lba, void *buf) {
    if (img_fd < 0) return -1;
    off_t off = (off_t)lba * 512;
    if (lseek(img_fd, off, SEEK_SET) != off) return -1;
    return read(img_fd, buf, 512) == 512 ? 0 : -1;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <ext2-image>\n", argv[0]);
        return 1;
    }

    img_fd = open(argv[1], O_RDONLY);
    if (img_fd < 0) { perror("open"); return 1; }

    block_read = host_block_read;

    if (ext2_mount()) {
        fprintf(stderr, "Mount failed\n");
        return 1;
    }

    printf("EXT2 mounted OK\n");

    // List root
    char names[256][EXT2_NAME_LEN + 1];
    int n = ext2_list_root(names, 256);
    printf("\nRoot directory (%d entries):\n", n);
    for (int i = 0; i < n; i++) {
        printf("  %s\n", names[i]);
    }

    // Open and read a known file
    ext2_fd_t fd;
    if (ext2_open("hello.txt", &fd) == 0) {
        char buf[4096];
        int r = ext2_read(&fd, buf, sizeof(buf) - 1);
        if (r > 0) {
            buf[r] = 0;
            printf("\nhello.txt contents:\n%s\n", buf);
        }
        ext2_close(&fd);
    }

    close(img_fd);
    return 0;
}
