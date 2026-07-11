; Stage1 — bootsector, loads stage2 from reserved area, far jumps to it
[org 0x7C00]
[BITS 16]

; ── FAT16 BPB (must match build_image.py) ──
jmp short start
nop
bpb_oem:      db "SKYBOOT "
bpb_bps:      dw 512
bpb_spc:      db 4
bpb_resv:     dw 65          ; 1 boot + 64 reserved for stage2
bpf_fatcnt:   db 2
bpb_rootmax:  dw 512
bpb_tot16:    dw 0
bpb_media:    db 0xF8
bpb_fat16sz:  dw 16          ; patched by build_image
bpb_spt:      dw 63
bpb_heads:    dw 16
bpb_hidden:   dd 0
bpb_tot32:    dd 32768       ; 16 MB — patched by build_image

; FAT32 extension — zeroed (unused in FAT16)
times 62-($-$$) db 0

; ── Boot code ──
start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    mov [boot_drive], dl

    ; Load stage2: LBA 1 .. bpb_resv-1
    mov ax, [bpb_resv]
    dec ax
    mov [dap_cnt], ax

    mov si, dap
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    jc disk_err

    jmp 0x0000:0x7E00

disk_err:
    mov si, msg
    call puts
.hlt: hlt; jmp .hlt

puts:
    mov ah, 0x0E
.l: lodsb; or al, al; jz .d; int 0x10; jmp .l
.d: ret

; ── Data ──
dap:      db 0x10, 0
dap_cnt:  dw 64
          dw 0x7E00, 0x0000
          dq 1

boot_drive: db 0
msg:        db "ERR", 0

times 510-($-$$) db 0
dw 0xAA55
