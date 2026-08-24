# ADR-021: VM Object Model

## Status

**Proposed** — No team decision required; follows from existing architecture.

## Context

Vahi has mmap, COW, and page cache implementations. This ADR formalizes the VM object model that ties them together.

Current state:
- mmap works (anonymous + file-backed)
- COW works (fork)
- Page cache exists (`vfs/page_cache.rs`)
- No formal VM object abstraction
- No dirty page tracking
- No writeback

## Decision

**Formalize the VM object model around existing implementations.**

### Architecture

```text
File / Inode
    ↓
Address Space / VM Object
    ↓
Page Cache
    ↓
Filesystem Mapping
    ↓
Block Layer
    ↓
Device
```

### New Types

```rust
/// A VM object represents a backing store for memory mappings.
pub enum VmObject {
    /// Anonymous memory (no backing file).
    Anonymous { pages: HashMap<u64, PhysFrame> },
    /// File-backed memory (backed by page cache).
    File { inode: Arc<dyn Inode>, cache: Arc<PageCache> },
    /// Shared memory (backed by shmem).
    Shared { id: u32, pages: HashMap<u64, PhysFrame> },
}

/// A memory mapping in a process's address space.
pub struct VmMapping {
    pub start: u64,
    pub end: u64,
    pub flags: VmFlags,
    pub object: VmObject,
    pub offset: u64,
    pub is_shared: bool,
}
```

### Acceptance Criteria

- [ ] VmObject type formalized
- [ ] Dirty page tracking works
- [ ] Writeback works
- [ ] Shared mappings work
- [ ] Private mappings work

## Consequences

### Positive

- Formal abstraction for mmap/COW/page cache
- Enables dirty page tracking and writeback
- Enables memory pressure handling

### Negative

- More types to maintain
- Migration cost (existing code uses ad-hoc structures)

## Alternatives Considered

### Alternative 1: Keep Ad-Hoc Structures

**Rejected.** No formal abstraction makes it hard to add features (dirty tracking, writeback).

### Alternative 2: Linux-Style vm_area_struct

**Adopted.** The VmMapping type is similar to Linux's vm_area_struct.
