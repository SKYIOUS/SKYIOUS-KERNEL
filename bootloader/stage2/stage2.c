// Stage2 — FAT16/32 loader with minimal menu, loads kernel.bin and jumps
// Compile:  gcc -m16 -march=i386 -ffreestanding -nostdlib -Os -fno-pic
//           -fno-stack-protector -fno-builtin -Wl,-T,linker.ld
//           -o stage2.elf stage2.c
// Binary:   objcopy -O binary stage2.elf stage2.bin

typedef unsigned char      u8;
typedef unsigned short     u16;
typedef unsigned int       u32;

#define NULL        ((void*)0)
#define SECTOR_SIZE 512
#define KERNEL_SEG  0x1000
#define KERNEL_OFF  0x0000
#define KERNEL_ADDR (((u32)KERNEL_SEG << 4) + KERNEL_OFF)

// boot_drive byte lives at 0x7DFC (stage1 stores it there before the AA55 sig)
#define BOOT_DRIVE (*(volatile u8 *)0x7DFC)

// Scratch sector buffer at a safe address below stage2 code
#define SECT_BUF   ((volatile u8 *)0x0600)

// ── FAT BPB (on-disk, packed) ──
struct __attribute__((packed)) BPB {
    u8  jmp[3];
    u8  oem[8];
    u16 bytes_per_sector;
    u8  sectors_per_cluster;
    u16 reserved_sectors;
    u8  fat_count;
    u16 root_max_entries;
    u16 total_sectors_16;
    u8  media_descriptor;
    u16 fat_size_16;
    u16 sectors_per_track;
    u16 head_count;
    u32 hidden_sectors;
    u32 total_sectors_32;
    u32 fat_size_32;
    u16 ext_flags;
    u16 fs_version;
    u32 root_cluster;
    u16 fs_info_sector;
    u16 backup_boot_sector;
};

// ── FAT directory entry ──
struct __attribute__((packed)) DirEntry {
    u8  name[11];
    u8  attr;
    u8  nt_reserved;
    u8  crt_time_tenth;
    u16 crt_time;
    u16 crt_date;
    u16 last_access;
    u16 cluster_hi;
    u16 wrt_time;
    u16 wrt_date;
    u16 cluster_lo;
    u32 file_size;
};

// ── Disk Address Packet for int 0x13 AH=0x42 ──
struct __attribute__((packed)) DAP {
    u8  size;       u8  zero;
    u16 count;
    u16 offset;     u16 segment;
    u32 lba_lo;     u32 lba_hi;
};

// ── helpers ──
static void putchar(u8 c) {
    __asm__ __volatile__("int $0x10" : : "a"(0x0E00 | c), "b"(7) : "cc");
}
static void puts(const char *s) { while (*s) putchar(*s++); }
static void puthex(u32 n) {
    int i; putchar('0'); putchar('x');
    for (i = 28; i >= 0; i -= 4) {
        u8 d = (n >> i) & 0xF;
        putchar(d < 10 ? '0'+d : 'A'+d-10);
    }
}

static void cls(void) {
    __asm__ __volatile__(
        "mov $0x0600, %%ax; xor %%cx, %%cx; mov $0x184F, %%dx; mov $0x07, %%bh; int $0x10"
        : : : "ax","cx","dx","bx","cc");
    __asm__ __volatile__(
        "mov $0x0200, %%ax; xor %%dx, %%dx; xor %%bx, %%bx; int $0x10"
        : : : "ax","dx","bx","cc");
}

static int getkey(void) {
    u16 key;
    __asm__ __volatile__("int $0x16" : "=a"(key) : : "cc");
    return key;
}

// ── disk I/O via int 0x13 AH=0x42 ──
static int read_sectors(u32 lba, u32 addr, u16 count) {
    struct DAP dap;
    dap.size    = 0x10;  dap.zero    = 0;
    dap.count   = count;
    dap.offset  = addr & 0xF;
    dap.segment = addr >> 4;
    dap.lba_lo  = lba;   dap.lba_hi  = 0;

    u16 si = (u16)(u32)(void*)&dap;
    u8 ok;
    __asm__ __volatile__(
        "movw %2, %%si\n"
        "movw $0x4200, %%ax\n"
        "int $0x13\n"
        "setnc %0\n"
        : "=qm"(ok)
        : "d"(BOOT_DRIVE), "rm"(si)
        : "ax", "si", "cc", "memory");
    return ok;
}
static int read_sector(u32 lba, u32 addr) { return read_sectors(lba, addr, 1); }

// ── FAT helpers ──
static int is_fat16(struct BPB *b) { return b->fat_size_16 != 0; }

static u32 fat_sector_count(struct BPB *b) {
    return is_fat16(b) ? b->fat_size_16 : b->fat_size_32;
}

static u32 root_dir_lba(struct BPB *b) {
    return b->reserved_sectors + b->fat_count * fat_sector_count(b);
}

static u32 root_dir_sectors(struct BPB *b) {
    return ((u32)b->root_max_entries * 32 + SECTOR_SIZE - 1) / SECTOR_SIZE;
}

static u32 data_start_lba(struct BPB *b) {
    return root_dir_lba(b) + root_dir_sectors(b);
}

static u32 cluster_to_lba(struct BPB *b, u32 cluster) {
    return data_start_lba(b) + (cluster - 2) * b->sectors_per_cluster;
}

static u32 next_cluster(struct BPB *b, u32 cur) {
    u32 fat_lba = b->reserved_sectors;
    if (is_fat16(b)) {
        if (!read_sector(fat_lba + (cur*2)/SECTOR_SIZE, (u32)SECT_BUF)) return 0;
        u16 v = *(volatile u16*)(SECT_BUF + (cur*2) % SECTOR_SIZE);
        return (v >= 0xFFF8) ? 0 : v;
    } else {
        if (!read_sector(fat_lba + (cur*4)/SECTOR_SIZE, (u32)SECT_BUF)) return 0;
        u32 v = *(volatile u32*)(SECT_BUF + (cur*4) % SECTOR_SIZE) & 0x0FFFFFFF;
        return (v >= 0x0FFFFFF8) ? 0 : v;
    }
}

// Convert "filename.ext" → 11-char FAT name
static void fat_name(const char *src, u8 *dst) {
    int i, j;
    for (i = 0; i < 11; i++) dst[i] = ' ';
    for (i = 0; src[i] && src[i] != '.' && i < 8; i++)
        dst[i] = (src[i] >= 'a' && src[i] <= 'z') ? src[i] - 32 : src[i];
    if (src[i] == '.') {
        ++i;
        for (j = 8; src[i] && j < 11; ++i, ++j)
            dst[j] = (src[i] >= 'a' && src[i] <= 'z') ? src[i] - 32 : src[i];
    }
}

// Find a file in FAT, return first cluster + size
static int find_file(struct BPB *b, const char *name,
                     u32 *out_cluster, u32 *out_size) {
    u8  want[11];
    int i, j;
    fat_name(name, want);

    u32 rlba   = root_dir_lba(b);
    u32 rsecs  = root_dir_sectors(b);
    u32 ecnt   = rsecs * (SECTOR_SIZE / 32);   // total directory entries

    if (is_fat16(b)) {
        for (i = 0; i < (int)ecnt; i++) {
            u32 sec = rlba + (i * 32) / SECTOR_SIZE;
            u32 off = (i * 32) % SECTOR_SIZE;
            if (off == 0 && !read_sector(sec, (u32)SECT_BUF)) return 0;

            volatile struct DirEntry *e = (volatile struct DirEntry *)(SECT_BUF + off);
            if (e->name[0] == 0)   return 0;
            if (e->name[0] == 0xE5) continue;

            int match = 1;
            for (j = 0; j < 11; j++)
                if (e->name[j] != want[j]) { match = 0; break; }
            if (match) {
                *out_cluster = e->cluster_lo;
                *out_size    = e->file_size;
                return 1;
            }
        }
    } else {
        // FAT32 — root dir is a cluster chain
        u32 cluster = b->root_cluster;
        u32 epc     = b->sectors_per_cluster * (SECTOR_SIZE / 32);
        int idx     = 0;
        while (cluster) {
            if (!read_sectors(cluster_to_lba(b, cluster), (u32)SECT_BUF,
                              b->sectors_per_cluster)) return 0;
            for (i = 0; i < (int)epc; i++) {
                volatile struct DirEntry *e =
                    (volatile struct DirEntry *)(SECT_BUF + i * sizeof(struct DirEntry));
                if (e->name[0] == 0)   return 0;
                if (e->name[0] == 0xE5) continue;
                int match = 1;
                for (j = 0; j < 11; j++)
                    if (e->name[j] != want[j]) { match = 0; break; }
                if (match) {
                    *out_cluster = ((u32)e->cluster_hi << 16) | e->cluster_lo;
                    *out_size    = e->file_size;
                    return 1;
                }
            }
            cluster = next_cluster(b, cluster);
        }
    }
    return 0;
}

// Load file given first cluster + size to a linear address
static int load_file(struct BPB *b, u32 cluster, u32 size, u32 addr) {
    u32 remain = size;
    while (cluster && remain > 0) {
        u32 chunk = b->sectors_per_cluster * SECTOR_SIZE;
        if (chunk > remain) chunk = remain;
        if (!read_sectors(cluster_to_lba(b, cluster), addr,
                          (chunk + SECTOR_SIZE - 1) / SECTOR_SIZE))
            return 0;
        addr   += chunk;
        remain -= chunk;
        cluster = next_cluster(b, cluster);
    }
    return remain == 0;
}

// ── entry point ──
void __attribute__((force_align_arg_pointer)) _start(void) {
    struct BPB bpb;
    u32 cluster, size;

    cls();
    puts("=== SKYOS Bootloader v0.1 ===\n\n");

    if (!read_sector(0, (u32)&bpb)) { puts("BPB read fail\n"); goto halt; }

    puts("FS: ");
    puts(is_fat16(&bpb) ? "FAT16" : "FAT32");
    puts("\n");

    // menu
    puts("\n[K] Boot kernel      [R] Reboot\n");
    for (;;) {
        puts("boot> ");
        u8 c = (u8)getkey();
        putchar(c); putchar('\n');
        if (c == 'K' || c == 'k') break;
        if (c == 'R' || c == 'r') __asm__ __volatile__("int $0x19" ::: "cc");
        puts("?\n");
    }

    puts("\nSearching KERNEL.BIN ...\n");
    if (!find_file(&bpb, "kernel.bin", &cluster, &size)) {
        puts("Not found\n"); goto halt;
    }
    puts("cluster="); puthex(cluster);
    puts("  size=");  puthex(size);
    puts("\n");

    if (!load_file(&bpb, cluster, size, KERNEL_ADDR)) {
        puts("Load fail\n"); goto halt;
    }
    puts("Loaded. Jumping ...\n");

    __asm__ __volatile__("ljmp %0, %1" : : "i"(KERNEL_SEG), "i"(KERNEL_OFF));

halt:
    puts("HALT\n");
    while (1) __asm__ __volatile__("hlt");
}
