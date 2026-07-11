# stage1.s — FAT16 VBR bootsector, loads stage2 from reserved area
# Syntax: GNU as with .intel_syntax
# Build:   as --32 -o stage1.o stage1.s && objcopy -O binary stage1.o stage1.bin

.code16
.intel_syntax noprefix

# FAT16 BPB (offsets must match FAT spec)
.org 0
    jmp  short real_start
    nop
    .byte 'S','K','Y','B','O','O','T',' '  # OEM
    .word 512                                # bytes per sector
    .byte 4                                  # sectors per cluster
    .word 65                                 # reserved sectors
    .byte 2                                  # FAT count
    .word 512                                # root max entries
    .word 0                                  # total sectors 16
    .byte 0xF8                               # media descriptor
    .word 16                                 # FAT16 size (patched)
    .word 63                                 # sectors per track
    .word 16                                 # head count
    .long 0                                  # hidden sectors
    .long 32768                              # total sectors 32

# Pad to byte 62 (start of boot code for FAT16)
.org 62

real_start:
    cli
    xor  ax, ax
    mov  ds, ax
    mov  es, ax
    mov  ss, ax
    mov  sp, 0x7C00
    sti

    mov  [boot_drive], dl

    # Load stage2: LBA 1, count = reserved_sectors - 1
    mov  ax, [0x0E]       # bpb_reserved_sectors
    dec  ax
    mov  [dap_cnt], ax

    mov  si, offset dap
    mov  ah, 0x42
    mov  dl, [boot_drive]
    int  0x13
    jc   disk_err

    jmp  0x0000:0x7E00

disk_err:
    mov  si, offset msg
    call puts
1:  hlt
    jmp  1b

puts:
    mov  ah, 0x0E
1:  lodsb
    test al, al
    jz   2f
    int  0x10
    jmp  1b
2:  ret

# DAP (Disk Address Packet)
.align 2
dap:
    .byte 0x10, 0
dap_cnt: .word 64
    .word 0x7E00, 0x0000
    .long 1, 0

boot_drive: .byte 0
msg: .asciz "ERR"

# Pad to 510, then boot signature
.org 510
    .word 0xAA55
