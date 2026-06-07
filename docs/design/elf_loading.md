# ELF Binary Loading Design

The kernel's ELF loader transforms executable files into running processes.

## Supported Formats

The loader handles both ELF32 and ELF64 executables for the x86_64 architecture. Shared libraries (ET_DYN) and position-independent executables (PIE) are supported.

## Loading Process

1. **Validation**: The kernel verifies the ELF magic (`\x7fELF`), class (64-bit), encoding (little-endian), and architecture (x86_64).

2. **Segment mapping**: Each `LOAD` segment is mapped into memory:
   - Program headers are read from the file
   - Memory regions are allocated for each segment
   - Segments are mapped with appropriate permissions (R, W, X)
   - Segments are aligned to page boundaries

```rust
for segment in elf.program_headers() {
    if segment.type() == PT_LOAD {
        let addr = segment.vaddr();
        let size = segment.memsz();
        let flags = segment.flags();
        task.vm.map_region(addr, size, flags)?;
        // Copy segment data from file
        reader.read_exact(&mut task.vm.data_mut(addr, size))?;
    }
}
```

3. **Stack setup**: The initial stack is populated with:
   - Argument strings (argv)
   - Environment strings (envp)
   - Auxiliary vector (AT_* entries for vDSO, page size, etc.)

4. **Entry point**: The CPU instruction pointer is set to the ELF entry point, and execution begins.

## Dynamic Linking

The kernel loads the dynamic linker (`ld-skyos.so`) for dynamically linked executables. The linker receives control first, resolves shared library dependencies, performs relocations, and then jumps to the program entry point.
