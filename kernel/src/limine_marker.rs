// Limine protocol request markers.
// global_asm! bypasses LTO — the assembler emits these bytes directly
// into the .limine_requests section. The Limine bootloader scans
// for these magic bytes to locate kernel requests.

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".section .limine_requests, \"aw\", @progbits",
    ".align 8",
    ".global _limine_requests_start",
    "_limine_requests_start:",
    ".quad 0xf6b8f4b39de7d1ae",
    ".quad 0xfab91a6940fcb9cf",
    ".quad 0x785c6ed015d3e316",
    ".quad 0x181e920a7852b9d9",
    ".global _limine_requests_end",
    "_limine_requests_end:",
    ".quad 0xadc0e0531bb10d03",
    ".quad 0x9572709f31764c62",
);
