#ifndef EXT2_H
#define EXT2_H

#include <stdint.h>

#define EXT2_NAME_LEN   255
#define EXT2_MAX_FILES  32

// ── On-disk structures (packed) ──

struct __attribute__((packed)) ext2_superblock {
    uint32_t inodes_count;
    uint32_t blocks_count;
    uint32_t r_blocks_count;
    uint32_t free_blocks_count;
    uint32_t free_inodes_count;
    uint32_t first_data_block;
    uint32_t log_block_size;
    uint32_t log_frag_size;
    uint32_t blocks_per_group;
    uint32_t frags_per_group;
    uint32_t inodes_per_group;
    uint32_t mtime;
    uint32_t wtime;
    uint16_t mnt_count;
    uint16_t max_mnt_count;
    uint16_t magic;
    uint16_t state;
    uint16_t errors;
    uint16_t minor_rev;
    uint32_t lastcheck;
    uint32_t checkinterval;
    uint32_t creator_os;
    uint32_t rev_level;
    uint16_t def_resuid;
    uint16_t def_resgid;
    // extended superblock follows for rev_level >= 1
};

struct __attribute__((packed)) ext2_bgdesc {
    uint32_t block_bitmap;
    uint32_t inode_bitmap;
    uint32_t inode_table;
    uint16_t free_blocks_count;
    uint16_t free_inodes_count;
    uint16_t used_dirs_count;
    uint16_t pad;
    uint32_t reserved[3];
};

#define EXT2_S_IFMT   0xF000
#define EXT2_S_IFDIR  0x4000
#define EXT2_S_IFREG  0x8000
#define EXT2_S_IFLNK  0xA000

struct __attribute__((packed)) ext2_inode {
    uint16_t mode;
    uint16_t uid;
    uint32_t size;
    uint32_t atime;
    uint32_t ctime;
    uint32_t mtime;
    uint32_t dtime;
    uint16_t gid;
    uint16_t links_count;
    uint32_t blocks;       // sectors (512) consumed
    uint32_t flags;
    uint32_t osd1;
    uint32_t block[15];    // 12 direct + 1 indirect + 1 dindirect + 1 tindirect
    uint32_t generation;
    uint32_t file_acl;
    uint32_t dir_acl;
    uint32_t faddr;
    uint32_t osd2[3];
};

struct __attribute__((packed)) ext2_dirent {
    uint32_t inode;
    uint16_t rec_len;
    uint8_t  name_len;
    uint8_t  file_type;
    char     name[];       // variable-length
};

// ── Vnode (in-memory) ──

typedef struct ext2_vnode {
    uint32_t ino;
    uint16_t mode;
    uint32_t size;
    uint32_t blocks[15];
} ext2_vnode_t;

typedef struct ext2_fd {
    ext2_vnode_t *vnode;
    uint32_t      pos;
} ext2_fd_t;

// ── API ──

int  ext2_mount(void);                            // + block device read function (set below)
int  ext2_open(const char *path, ext2_fd_t *fd);
int  ext2_read(ext2_fd_t *fd, void *buf, uint32_t len);
int  ext2_close(ext2_fd_t *fd);
int  ext2_list_root(char entries[][EXT2_NAME_LEN+1], int max);

// Set these before calling ext2_mount():
extern int (*block_read)(uint32_t lba, void *buf);

#endif
