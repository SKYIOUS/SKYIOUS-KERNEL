# ADR-014: Page Cache

## Status
Accepted

## Date
2026-08-15

## Context
The kernel needs a page cache to avoid repeated disk I/O for frequently accessed file data. The VFS layer exposes `VfsNode` traits but has no unified caching layer between filesystems and block devices.

## Decision
Implement a page cache integrated with the VFS and buddy allocator.

### Design
- Page cache maps (file, page_index) → physical frame
- LRU eviction with configurable high/low watermarks
- Writeback support for dirty pages (async + sync)
- Integration with mmap for file-backed mappings
- Readahead for sequential access patterns

### Acceptance Criteria
- [ ] Page cache reduces disk I/O for repeated reads
- [ ] mmap file-backed mappings work via page cache
- [ ] Writeback preserves data integrity on shutdown/crash
- [ ] LRU eviction prevents memory pressure

## Consequences
- Adds ~1000 lines to VFS/memory subsystem
- Requires coordination with swap subsystem
- Must handle concurrent access from multiple processes

## Alternatives Considered
### No page cache (direct I/O only)
- Pros: Simpler, no cache coherency issues
- Cons: Severe performance penalty, no mmap support
- Rejected: Modern kernels require page cache

### Per-filesystem caching
- Pros: Filesystem-specific optimization
- Cons: Duplicated code, no unified memory pressure handling
- Rejected: Single page cache serves all filesystems