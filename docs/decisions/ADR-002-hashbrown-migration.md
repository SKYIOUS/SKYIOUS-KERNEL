# ADR-002: Hashbrown + ahash for HashMap-based Data Structures

## Status
Accepted

## Date
2026-08-01

## Context
The kernel initially used `BTreeMap` from `alloc::collections` for process tables, handle tables, and other map-like data structures. As the kernel grew, several issues emerged:
- BTreeMap O(log n) lookups became measurable in hot paths (syscall dispatch, handle resolution)
- Tree structure overhead per entry (~3 pointers vs hashmap's ~1)
- No ability to control the hash function for DoS resistance
- BTreeMap iteration order guarantees were unused

## Decision
Replace BTreeMap with `hashbrown` (Rust port of Google's SwissTable) using `ahash` for hashing.

```toml
hashbrown = { version = "0.14", default-features = false, features = ["alloc", "ahash"] }
```

## Alternatives Considered

### Keep BTreeMap
- Pros: No dependency change, ordered iteration, stable O(log n) worst-case
- Cons: Slower average-case, no hash-flood resistance
- Rejected: Performance gap widened as process counts grew

### Custom hash table
- Pros: Full control, no external dependency
- Cons: Maintenance burden, subtle correctness issues
- Rejected: hashbrown is well-tested and no_std compatible

### std HashMap
- Pros: Familiar API
- Cons: Not available in no_std, SipHash is slow for kernel workloads
- Rejected: no_std prevents std HashMap usage

## Consequences
- O(1) average-case lookups in hot paths
- ahash provides DoS-resistant hashing with minimal overhead (~1 cycle/byte)
- hashbrown is no_std compatible with the `alloc` feature
- Remaining BTreeMap in PROCESS_TABLE is acceptable (low-update, low-QPS path)
- All kernel HashMaps now use hashbrown (see Cargo.toml dependency)