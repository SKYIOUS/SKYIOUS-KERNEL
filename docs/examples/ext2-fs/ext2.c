// ── Minimal EXT2 read-only driver ──
// Supports: mount, open-by-path, read, close, list-root.
// ponytail: read-only, no symlink/unlink/write; add when needed.

#include "ext2.h"
#include <stddef.h>
#include <string.h>

// ── Block-device callback ──
int (*block_read)(uint32_t lba, void *buf);

// ─── internal state ───
#define SB_BUF_SIZE 1024
static uint8_t sb_buf[SB_BUF_SIZE];          // full superblock buffer (1024 B)
#define sb ((struct ext2_superblock *)sb_buf)

static uint32_t  block_size;
static uint32_t  block_mask;
static uint32_t  inode_size = 128;     // minimum; rev >= 1 may be larger
static uint32_t  bg_desc_per_block;
static uint32_t  inodes_per_block;
static uint32_t  num_bg;
static uint32_t  first_bg_lba;         // block group descriptor table start

static ext2_vnode_t vnode_pool[EXT2_MAX_FILES];
static int          vnode_count = 0;

// ─── helpers ───

static int read_block(uint32_t blk, void *buf) {
    return block_read(blk * (block_size / 512), buf);
}

static uint32_t bgdesc_lba(uint32_t bg) {
    return first_bg_lba + bg / bg_desc_per_block;
}
static uint32_t bgdesc_off(uint32_t bg) {
    return (bg % bg_desc_per_block) * sizeof(struct ext2_bgdesc);
}

static int get_bgdesc(uint32_t bg, struct ext2_bgdesc *bgd) {
    // ponytail: reads the block each time; cache the BGDesc block for speed
    uint8_t tmp[4096];   // max block size (assume <= 4096)
    if (read_block(bgdesc_lba(bg), tmp)) return -1;
    memcpy(bgd, tmp + bgdesc_off(bg), sizeof(*bgd));
    return 0;
}

static int read_inode(uint32_t ino, struct ext2_inode *in) {
    uint32_t bg = (ino - 1) / sb->inodes_per_group;
    uint32_t idx = (ino - 1) % sb->inodes_per_group;
    struct ext2_bgdesc bgd;
    if (get_bgdesc(bg, &bgd)) return -1;
    uint32_t tbl_lba = bgd.inode_table;
    uint32_t blk = tbl_lba + (idx * inode_size) / block_size;
    uint32_t off = (idx * inode_size) % block_size;
    uint8_t tmp[4096];
    if (read_block(blk, tmp)) return -1;
    memcpy(in, tmp + off, sizeof(*in));
    return 0;
}

// ─── block pointer resolution (direct + indirect) ───

static uint32_t resolve_block(const uint32_t *blocks, uint32_t logical) {
    uint32_t ptrs_per_block = block_size / 4;
    uint32_t levels[4] = {
        12,                              // direct
        ptrs_per_block,                  // singly indirect
        ptrs_per_block * ptrs_per_block, // doubly indirect
        ptrs_per_block * ptrs_per_block * ptrs_per_block, // triply indirect
    };

    uint32_t base = 0;
    for (int l = 0; l < 4; l++) {
        if (logical < base + levels[l]) {
            uint32_t idx = logical - base;
            if (l == 0) {
                return blocks[idx];      // direct block
            }
            // Read indirect block
            uint32_t ind_lba = blocks[11 + l];  // block[12], [13], [14]
            uint32_t ind_buf[4096 / 4];
            if (read_block(ind_lba, ind_buf)) return 0;
            // Walk down chain
            for (int d = l; d > 1; d--) {
                uint32_t per = ptrs_per_block;
                for (int p = 1; p < d; p++) per *= ptrs_per_block;
                uint32_t sub = idx / per;
                idx %= per;
                if (read_block(ind_buf[sub], ind_buf)) return 0;
            }
            return ind_buf[idx];
        }
        base += levels[l];
    }
    return 0;  // beyond max file size
}

// ─── directory search ───

static int find_inode(uint32_t dir_ino, const char *name, uint32_t *out_ino) {
    struct ext2_inode in;
    if (read_inode(dir_ino, &in)) return -1;

    uint32_t blocks[15];
    memcpy(blocks, in.block, sizeof(blocks));   // copy to avoid packed-member address warning

    uint32_t pos = 0;
    uint8_t buf[4096];

    while (pos < in.size) {
        uint32_t logical = pos / block_size;
        uint32_t off     = pos % block_size;
        uint32_t phys    = resolve_block(blocks, logical);
        if (!phys) return -1;

        if (read_block(phys, buf)) return -1;

        while (off < block_size) {
            struct ext2_dirent *de = (struct ext2_dirent *)(buf + off);
            if (de->inode == 0) { off += de->rec_len; continue; }

            uint32_t nlen = de->name_len;
            if (nlen == strlen(name) &&
                memcmp(de->name, name, nlen) == 0) {
                *out_ino = de->inode;
                return 0;
            }
            off += de->rec_len;
            if (de->rec_len == 0) break;  // safety
        }
        pos += block_size - (pos % block_size ? pos % block_size : 0);
        if (pos % block_size) pos = (pos / block_size + 1) * block_size;
    }
    return -1;  // not found
}

// ─── public API ───

int ext2_mount(void) {
    if (!block_read) return -1;

    // Read superblock (2 KB: block 1 occupies LBA 2..3 for 1024 B blocks)
    if (block_read(2, sb_buf) || block_read(3, sb_buf + 512)) return -1;

    if (sb->magic != 0xEF53) return -1;

    block_size = 1024 << sb->log_block_size;
    block_mask = block_size - 1;
    if (sb->rev_level >= 1) {
        // s_inode_size at offset 128 + 64 = 192 in the extended superblock
        inode_size = *(uint16_t *)(sb_buf + 192);
        if (inode_size < 128) inode_size = 128;
    }

    bg_desc_per_block = block_size / sizeof(struct ext2_bgdesc);
    inodes_per_block  = block_size / inode_size;
    num_bg = (sb->inodes_count + sb->inodes_per_group - 1) / sb->inodes_per_group;

    // First BGDesc block is block 2 (block_size=1024) or block 1 (larger)
    first_bg_lba = (block_size == 1024) ? 2 : 1;

    return 0;
}

int ext2_open(const char *path, ext2_fd_t *fd) {
    // Skip leading /
    while (*path == '/') path++;
    if (*path == 0) return -1;

    uint32_t current_ino = 2;           // root inode is always 2

    char component[EXT2_NAME_LEN + 1];
    int  ci = 0;

    while (1) {
        if (*path == '/' || *path == 0) {
            if (ci == 0) { path++; continue; }
            component[ci] = 0;

            uint32_t next_ino;
            if (find_inode(current_ino, component, &next_ino)) return -1;

            if (*path == 0) {
                // Final component
                struct ext2_inode in;
                if (read_inode(next_ino, &in)) return -1;

                if (vnode_count >= EXT2_MAX_FILES) return -1;
                ext2_vnode_t *vn = &vnode_pool[vnode_count++];
                vn->ino  = next_ino;
                vn->mode = in.mode;
                vn->size = in.size;
                { uint32_t blk[15]; memcpy(blk, in.block, sizeof(blk)); memcpy(vn->blocks, blk, sizeof(vn->blocks)); }

                fd->vnode = vn;
                fd->pos   = 0;
                return 0;
            }

            current_ino = next_ino;
            ci = 0;
            path++;
            continue;
        }

        if (ci < EXT2_NAME_LEN) component[ci++] = *path;
        path++;
    }
}

int ext2_read(ext2_fd_t *fd, void *buf, uint32_t len) {
    ext2_vnode_t *vn = fd->vnode;
    if (fd->pos >= vn->size) return 0;

    uint32_t remain = vn->size - fd->pos;
    if (len > remain) len = remain;

    uint8_t *dst = (uint8_t *)buf;
    while (len > 0) {
        uint32_t logical = fd->pos / block_size;
        uint32_t off     = fd->pos % block_size;
        uint32_t chunk   = block_size - off;
        if (chunk > len) chunk = len;

        uint32_t phys = resolve_block(vn->blocks, logical);
        if (!phys) return -1;

        uint8_t tmp[4096];
        if (read_block(phys, tmp)) return -1;

        memcpy(dst, tmp + off, chunk);
        dst += chunk;
        fd->pos += chunk;
        len -= chunk;
    }
    return dst - (uint8_t *)buf;
}

int ext2_close(ext2_fd_t *fd) {
    // ponytail: no-op; slab allocator would free here
    fd->vnode = NULL;
    fd->pos = 0;
    return 0;
}

int ext2_list_root(char entries[][EXT2_NAME_LEN + 1], int max) {
    int count = 0;
    struct ext2_inode root_in;
    if (read_inode(2, &root_in)) return -1;

    uint32_t blocks[15];
    memcpy(blocks, root_in.block, sizeof(blocks));

    uint32_t pos = 0;
    uint8_t buf[4096];

    while (pos < root_in.size) {
        uint32_t logical = pos / block_size;
        uint32_t off     = pos % block_size;
        uint32_t phys    = resolve_block(blocks, logical);
        if (!phys) break;

        if (read_block(phys, buf)) break;

        while (off < block_size && count < max) {
            struct ext2_dirent *de = (struct ext2_dirent *)(buf + off);
            if (de->inode == 0) { off += de->rec_len; continue; }

            uint32_t nlen = de->name_len;
            if (nlen > EXT2_NAME_LEN) nlen = EXT2_NAME_LEN;
            memcpy(entries[count], de->name, nlen);
            entries[count][nlen] = 0;
            count++;

            off += de->rec_len;
            if (de->rec_len == 0) break;
        }
        pos += block_size;
    }
    return count;
}
