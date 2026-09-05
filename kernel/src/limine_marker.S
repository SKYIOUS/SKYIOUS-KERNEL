// Limine protocol request markers.
// Placed in a dedicated section to survive LTO and dead-code elimination.
// The Limine bootloader scans for these magic bytes to locate kernel requests.

.section .limine_requests, "aw", @progbits

// RequestsStartMarker: 4 x u64
.align 8
.global _limine_requests_start
_limine_requests_start:
    .quad 0xf6b8f4b39de7d1ae
    .quad 0xfab91a6940fcb9cf
    .quad 0x785c6ed015d3e316
    .quad 0x181e920a7852b9d9
