// Minimal test for ET_DYN with split address space
// Tests: can we have dynamic sections at low address and kernel at higher-half?

void _start(void) {
    volatile int x = 0x12345678;
    (void)x;
}

const long rodata_val = 0xdeadbeef;
long data_val = 0xcafebabe;
long bss_val;