# stage2.s — FAT16/32 bootloader: menu, find kernel.bin, load, jump
# Syntax: GNU as with .intel_syntax (no prefix)
# Build:   as --32 -o stage2.o stage2.s && objcopy -O binary stage2.o stage2.bin
# Expects to be loaded at 0x0000:0x7E00 by stage1.

.code16gcc
.intel_syntax noprefix

# ─── Constants ───
.equ SECTOR_SIZE,      512
.equ KERNEL_SEG,       0x1000
.equ KERNEL_OFF,       0x0000
.equ SECT_BUF,         0x0600          # scratch sector buffer (below us)
.equ BOOT_DRIVE_ADDR,  0x7DFC          # stage1 stores DL here

# ─── Entry ───
.globl _start
_start:
    # Set up segments & stack (safety; stage1 already did this)
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, 0x7C00

    call cls

    mov  si, msg_banner
    call puts

    # Read BPB (sector 0 → SECT_BUF)
    mov  eax, 0
    mov  bx, SECT_BUF
    call read_sector
    test al, al
    jz   .fail

    # Detect FAT variant
    mov  ax, [SECT_BUF + 0x16]         # bpb_fat_size_16
    test ax, ax
    jnz  .fat16
    mov  si, msg_fat32
    call puts
    jmp  .menu
.fat16:
    mov  si, msg_fat16
    call puts

.menu:
    mov  si, msg_menu
    call puts

.prompt_loop:
    mov  si, msg_prompt
    call puts
    call getkey
    mov  byte [.keybuf], al
    call putchar
    call newline

    cmp  byte [.keybuf], 'k'
    je   .load
    cmp  byte [.keybuf], 'K'
    je   .load
    cmp  byte [.keybuf], 'r'
    je   reboot
    cmp  byte [.keybuf], 'R'
    je   reboot
    mov  si, msg_unknown
    call puts
    jmp  .prompt_loop

.load:
    mov  si, msg_loading
    call puts

    call find_kernel
    test al, al
    jz   .not_found

    # Load kernel: cluster in cx, size in edi
    call load_kernel
    test al, al
    jz   .load_fail

    mov  si, msg_jump
    call puts

    # Far jump to kernel
    ljmp KERNEL_SEG, KERNEL_OFF

.not_found:
    mov  si, msg_nf
    call puts
    jmp  .halt

.load_fail:
    mov  si, msg_lf
    call puts

.halt:
    mov  si, msg_halt
    call puts
1:  hlt
    jmp  1b

.fail:
    mov  si, msg_bpb_fail
    call puts
    jmp  .halt

.data
.keybuf: .byte 0

# ─── FAT parsing helpers ───

# find_kernel: search root directory for "KERNEL  BIN"
# Returns: al=1 if found, cx=first_cluster, edi=file_size
find_kernel:
    push bx
    push si
    push di

    # Compute root dir LBA
    mov  ax, [SECT_BUF + 0x10]         # reserved_sectors
    mov  bx, [SECT_BUF + 0x16]         # fat_size_16
    test bx, bx
    jnz  .fat16_2
    mov  bx, [SECT_BUF + 0x24]         # fat_size_32
.fat16_2:
    mov  cl, [SECT_BUF + 0x10 + 1]      # ... wait, need fat_count
    # Actually: root_dir_lba = reserved + fat_count * fat_size
    mov  cl, [SECT_BUF + 0x10]         # fat_count at offset 0x10

    xor  ch, ch
    xor  dh, dh
    .loop:
        add  ax, bx
        dec  cx
        jnz  .loop
    # ax = root_dir_lba (16-bit is fine for small images)

    mov  [.root_lba], ax

    # Root dir sectors
    mov  bx, [SECT_BUF + 0x11]         # root_max_entries
    shr  bx, 4                         # (root_max * 32) / 512 = root_max / 16
    mov  [.root_secs], bx

    # Iterate over root dir entries
    xor  di, di                         # entry index
    xor  cx, cx                         # sector within root dir
.rd_loop:
    cmp  di, [.total_entries]
    jae  .notfound

    mov  ax, cx
    mov  bx, SECT_BUF
    call read_sector
    test al, al
    jz   .notfound

    # Check each entry in this sector
    xor  si, si                         # offset within sector
.entry_loop:
    cmp  si, SECTOR_SIZE
    jae  .next_sector

    # Compare FAT name with "KERNEL  BIN"
    mov  al, [SECT_BUF + si]
    cmp  al, 0
    je   .notfound                       # end of directory
    cmp  al, 0xE5
    je   .skip                           # deleted entry

    push si
    add  si, SECT_BUF
    mov  di, si
    mov  si, kernel_name
    push cx
    mov  cx, 11
    cld
    repe cmpsb
    pop  cx
    pop  si
    je   .found_entry

.skip:
    add  si, 32
    inc  di
    jmp  .entry_loop

.next_sector:
    inc  cx
    jmp  .rd_loop

.found_entry:
    # Get cluster and size
    mov  cx, [SECT_BUF + si + 0x1A]     # cluster_lo
    mov  edi, [SECT_BUF + si + 0x1C]    # file_size
    # For FAT32 also check cluster_hi
    mov  ax, [SECT_BUF + 0x16]
    test ax, ax
    jnz  .got
    # FAT32: combine hi+lo
    mov  ax, [SECT_BUF + si + 0x14]     # cluster_hi
    shl  eax, 16
    and  ecx, 0xFFFF
    or   ecx, eax
.got:
    mov  [.cluster], cx
    mov  [.size], edi
    mov  al, 1
    pop  di
    pop  si
    pop  bx
    ret

.notfound:
    xor  al, al
    pop  di
    pop  si
    pop  bx
    ret

.data
.cluster: .word 0
.size:    .long 0
.root_lba: .word 0
.root_secs: .word 0
.total_entries: .word 0

# load_kernel: load kernel file to KERNEL_ADDR
# Input: cx=first_cluster, edi=file_size
# Returns: al=1 on success
load_kernel:
    push bx
    push si
    push di
    push cx
    push edi

    mov  [.lcluster], cx
    mov  [.lsize], edi
    mov  [.ldest], KERNEL_SEG * 16 + KERNEL_OFF

.loop:
    mov  cx, [.lcluster]
    test cx, cx
    jz   .ldone

    # Convert cluster to LBA
    xor  eax, eax
    mov  ax, cx
    call cluster_to_lba

    # Read cluster worth of sectors
    mov  cl, [SECT_BUF + 0x0D]         # sectors_per_cluster
    xor  ch, ch
    mov  bx, [.ldest]
    mov  [.saved_dest], bx

    # Calculate how many sectors to read
    mov  edi, [.lsize]
    mov  ebx, SECTOR_SIZE
    mov  eax, [.sectors_per_cluster_bytes]
    cmp  edi, eax
    jae  .read_full_cluster
    # Partial cluster — round up to sectors
    mov  eax, edi
    add  eax, SECTOR_SIZE - 1
    xor  edx, edx
    div  ebx
    mov  cx, ax
    mov  eax, [.lba_result]
    jmp  .do_read
.read_full_cluster:
    mov  cx, [SECT_BUF + 0x0D]         # sectors_per_cluster
    mov  eax, [.lba_result]
.do_read:
    mov  bx, [.ldest]
    push eax
    call read_sectors
    pop  eax
    test al, al
    jz   .lfail

    # Advance destination
    mov  cx, [SECT_BUF + 0x0D]
    xor  ch, ch
    mov  eax, cx
    mov  ebx, SECTOR_SIZE
    mul  ebx
    add  [.ldest], eax

    # Subtract from remaining size
    mov  eax, [.lsize]
    sub  eax, [.sectors_per_cluster_bytes]
    jbe  .ldone
    mov  [.lsize], eax

    # Get next cluster in chain
    mov  cx, [.lcluster]
    call next_cluster
    mov  [.lcluster], ax
    test ax, ax
    jnz  .loop
    jmp  .ldone

.ldone:
    mov  al, 1
    pop  edi
    pop  cx
    pop  di
    pop  si
    pop  bx
    ret

.lfail:
    xor  al, al
    pop  edi
    pop  cx
    pop  di
    pop  si
    pop  bx
    ret

.data
.lcluster: .word 0
.lsize:    .long 0
.ldest:    .long 0
.saved_dest: .word 0
.lba_result: .long 0
.sectors_per_cluster_bytes: .long 0

# cluster_to_lba: convert cluster number to LBA
# Input: eax = cluster number
# Output: eax = LBA; [.lba_result] = LBA (saved)
cluster_to_lba:
    push bx
    push cx
    # Compute data_start_lba
    xor  eax, eax
    mov  ax, [SECT_BUF + 0x0E]         # reserved_sectors
    xor  ebx, ebx
    mov  bl, [SECT_BUF + 0x10]         # fat_count
    xor  ecx, ecx
    mov  cx, [SECT_BUF + 0x16]         # fat_size_16
    test cx, cx
    jnz  .cl_calc
    mov  cx, [SECT_BUF + 0x24]         # fat_size_32
.cl_calc:
    xor  edx, edx
.cl_loop:
    add  eax, ecx
    dec  ebx
    jnz  .cl_loop

    # + root_dir_sectors
    mov  bx, [SECT_BUF + 0x11]         # root_max_entries
    shr  bx, 4                         # root_max / 16 = sectors
    add  ax, bx

    # + (cluster - 2) * sectors_per_cluster
    mov  ecx, [.cl_input]
    sub  ecx, 2
    jb   .cl_err
    xor  ebx, ebx
    mov  bl, [SECT_BUF + 0x0D]         # sectors_per_cluster
    mov  eax, ecx
    mul  ebx
    add  eax, [.cl_base]
    mov  [.lba_result], eax
    pop  cx
    pop  bx
    ret
.cl_err:
    xor  eax, eax
    mov  [.lba_result], eax
    pop  cx
    pop  bx
    ret

.data
.cl_input:   .long 0
.cl_base:    .long 0

# next_cluster: read FAT entry for given cluster
# Input: cx=cluster number
# Output: ax=next_cluster (0 if EOC or error)
next_cluster:
    push bx
    push cx
    push si

    mov  ax, [SECT_BUF + 0x16]         # fat_size_16
    test ax, ax
    jnz  .nc_fat16

    # FAT32
    mov  eax, ecx
    xor  edx, edx
    mov  ecx, 4
    mul  ecx                            # eax = cluster * 4
    mov  ecx, SECTOR_SIZE
    div  ecx                            # eax = sector offset, edx = byte offset
    add  eax, [SECT_BUF + 0x0E]        # + reserved_sectors = FAT LBA
    mov  bx, SECT_BUF
    call read_sector
    test al, al
    jz   .nc_err
    mov  eax, [SECT_BUF + edx]
    and  eax, 0x0FFFFFFF
    cmp  eax, 0x0FFFFFF8
    jae  .nc_eoc
    jmp  .nc_done

.nc_fat16:
    mov  eax, ecx
    xor  edx, edx
    mov  ecx, 2
    mul  ecx                            # eax = cluster * 2
    mov  ecx, SECTOR_SIZE
    div  ecx                            # eax = sector offset, edx = byte offset
    add  eax, [SECT_BUF + 0x0E]        # + reserved_sectors
    mov  bx, SECT_BUF
    call read_sector
    test al, al
    jz   .nc_err
    mov  ax, [SECT_BUF + edx]
    cmp  ax, 0xFFF8
    jae  .nc_eoc

.nc_done:
    pop  si
    pop  cx
    pop  bx
    ret

.nc_eoc:
    xor  eax, eax
    pop  si
    pop  cx
    pop  bx
    ret

.nc_err:
    xor  eax, eax
    pop  si
    pop  cx
    pop  bx
    ret

# ─── Disk I/O ───

# read_sector(eax=LBA, bx=buffer) → al=1 ok
read_sector:
    push si
    push cx
    push bx
    mov  word [.dap_count], 1
    mov  [.dap_lba], eax
    mov  [.dap_off], bx
    mov  si, dap
    mov  ah, 0x42
    mov  dl, [BOOT_DRIVE_ADDR]
    int  0x13
    setnc al
    pop  bx
    pop  cx
    pop  si
    ret

# read_sectors(eax=LBA, bx=buffer, cx=count) → al=1 ok
read_sectors:
    push si
    mov  [.dap_count], cx
    mov  [.dap_lba], eax
    mov  [.dap_off], bx
    mov  si, dap
    mov  ah, 0x42
    mov  dl, [BOOT_DRIVE_ADDR]
    int  0x13
    setnc al
    pop  si
    ret

.data
dap:
.dap_size:  .byte 0x10
.dap_res:   .byte 0
.dap_count: .word 0
.dap_off:   .word 0
.dap_seg:   .word 0
.dap_lba:   .long 0
            .long 0

# ─── Text I/O ───

putchar:
    push bx
    mov  ah, 0x0E
    mov  bx, 7
    int  0x10
    pop  bx
    ret

puts:
    push ax
    push si
.l: lodsb
    test al, al
    jz  .d
    call putchar
    jmp .l
.d: pop si
    pop ax
    ret

newline:
    push ax
    mov  al, 0x0D
    call putchar
    mov  al, 0x0A
    call putchar
    pop  ax
    ret

cls:
    push ax
    push bx
    push cx
    push dx
    mov  ax, 0x0600
    xor  cx, cx
    mov  dx, 0x184F
    mov  bh, 7
    int  0x10
    mov  ax, 0x0200
    xor  dx, dx
    xor  bx, bx
    int  0x10
    pop  dx
    pop  cx
    pop  bx
    pop  ax
    ret

getkey:
    xor  ax, ax
    int  0x16
    ret

reboot:
    int  0x19

# ─── Strings ───
msg_banner:  .asciz "=== SKYOS Bootloader v0.1 ===\n\n"
msg_fat16:   .asciz "FS: FAT16\n"
msg_fat32:   .asciz "FS: FAT32\n"
msg_menu:    .asciz "\n[K] Boot kernel    [R] Reboot\n\n"
msg_prompt:  .asciz "boot> "
msg_unknown: .asciz "?\n"
msg_loading: .asciz "\nLoading KERNEL.BIN ...\n"
msg_nf:      .asciz "Not found\n"
msg_lf:      .asciz "Load failed\n"
msg_jump:    .asciz "Loaded. Jumping ...\n"
msg_halt:    .asciz "HALT\n"
msg_bpb_fail:.asciz "BPB read fail\n"

kernel_name: .asciz "KERNEL  BIN"
