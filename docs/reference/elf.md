# ELF Format Reference for Kernel Loading

SkyOS uses the ELF64 format for kernel and userspace binaries.

## ELF Header

```c
typedef struct {
    unsigned char e_ident[16];  // Magic and class info
    uint16_t e_type;            // ET_EXEC (2) or ET_DYN (3)
    uint16_t e_machine;         // EM_X86_64 (62)
    uint32_t e_version;         // EV_CURRENT (1)
    uint64_t e_entry;           // Entry point virtual address
    uint64_t e_phoff;           // Program header offset
    uint64_t e_shoff;           // Section header offset
    uint32_t e_flags;           // Processor-specific flags
    uint16_t e_ehsize;          // ELF header size (64)
    uint16_t e_phentsize;       // Program header entry size (56)
    uint16_t e_phnum;           // Number of program headers
    uint16_t e_shentsize;       // Section header entry size (64)
    uint16_t e_shnum;           // Number of section headers
    uint16_t e_shstrndx;        // Section header string table index
} Elf64_Ehdr;
```

## Program Headers

```c
typedef struct {
    uint32_t p_type;    // PT_LOAD (1), PT_DYNAMIC (2), PT_INTERP (3)
    uint32_t p_flags;   // PF_R (4), PF_W (2), PF_X (1)
    uint64_t p_offset;  // Offset in file
    uint64_t p_vaddr;   // Virtual address
    uint64_t p_paddr;   // Physical address (unused)
    uint64_t p_filesz;  // Size in file
    uint64_t p_memsz;   // Size in memory
    uint64_t p_align;   // Alignment (usually page size)
} Elf64_Phdr;
```

## Segment Loading

The kernel loader maps each `PT_LOAD` segment:
- File data is read from `p_offset` for `p_filesz` bytes
- The segment is placed at `p_vaddr` (page-aligned)
- Memory beyond `p_filesz` up to `p_memsz` is zero-filled (BSS)
- Permissions are set from `p_flags`

## Dynamic Linking

`PT_DYNAMIC` segments contain:
- `DT_NEEDED`: Required shared libraries
- `DT_SYMTAB`/`DT_STRTAB`: Symbol tables
- `DT_RELA`/`DT_REL`: Relocation entries
- `DT_INIT`/`DT_FINI`: Init/fini functions

## ELF Identification

The kernel validates the ELF magic bytes:
```
0x7f, 'E', 'L', 'F'
```
Checks:
- Class: ELF64 (2)
- Encoding: Little-endian (1)
- Version: EV_CURRENT (1)
- OS/ABI: ELFOSABI_NONE (0) or ELFOSABI_SYSV (0)
