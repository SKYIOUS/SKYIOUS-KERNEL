# Phase 4: Userspace Ecosystem

Phase 4 focuses on building a complete userspace environment.

## Goals

- Init system with service management
- Package manager
- Shell with scripting support
- Core utilities (ls, cat, cp, mv, rm, ps, top)
- Text editor
- C compiler toolchain

## Key Milestones

1. **Init system**: Service lifecycle management, dependency resolution, logging
2. **Package manager**: Binary package format, repository support, dependency resolution
3. **Shell**: Command parsing, job control, pipes, redirections, environment variables
4. **Coreutils**: POSIX-compatible implementations of essential utilities
5. **Editor**: A modal text editor with syntax highlighting
6. **Toolchain**: Port of GCC or integration with LLVM for native compilation

## Userspace Libraries

Development of shared libraries:
- `libc`: Standard C library (continued expansion)
- `libgui`: GUI application toolkit
- `libnet`: Networking helper library
- `libpthread`: POSIX threads implementation

## Expected Timeline

4-6 months (ongoing parallel effort).
