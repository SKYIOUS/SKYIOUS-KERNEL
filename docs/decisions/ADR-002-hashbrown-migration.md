# ADR-002: Hashbrown + ahash for HashMap-based Data Structures

## Status
Accepted
Amended 2026-09-25 (0.17 migration for `-Z build-std` compatibility)

## Date
2026-08-01

## Context
The kernel initially used `BTreeMap` from `alloc::collections` for process tables, handle tables, and other map-like data structures. As the kernel grew, several issues emerged:
- BTreeMap O(log n) lookups became measurable in hot paths (syscall dispatch, handle resolution)
- Tree structure overhead per entry (~3 pointers vs hashmap's ~1)
- No ability to control the hash function for DoS resistance
- BTreeMap iteration order guarantees were unused

## Decision
Replace BTreeMap with `hashbrown` (Rust port of Google's SwissTable) using a
fast default hasher. The kernel uses only `hashbrown::HashMap`/`HashMap::new()`,
so the ahash-backed default is sufficient.

```toml
hashbrown = { version = "0.17", default-features = false, features = ["default-hasher"] }
```
`default-features = false` keeps the crate lean; `default-hasher` pulls `foldhash`
and provides the `DefaultHashBuilder` required by `HashMap::new()`.

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

### std::hash::BuildHasher (rejected)
- Same DoS-exposure reason as BTreeMap.

### CI build-std compatibility (amendment 2026-09-25)
The `t00-harness` pipeline builds the kernel with `-Z build-std=core,alloc`
(rebuilt `core`/`alloc` from source). Under that flag, `hashbrown 0.14.x` failed CI
with `E0464: multiple candidates for 'rmeta' dependency 'alloc'`: hashbrown 0.14
unconditionally executes `extern crate alloc;` (src/lib.rs) and ships a
`rustc-std-workspace-alloc` shim dependency; Cargo resolved the bare feature name
`alloc` to hashbrown 0.14's optional `rustc-std-workspace-alloc` dependency, so
`extern crate alloc;` was ambiguous between the build-std real `alloc`
(candidate #1) and the shim (candidate #2). `hashbrown 0.17` made the shim an
optional dependency gated by `rustc-dep-of-std` (which the kernel never enables)
and replaced the unconditional shim with `extern crate alloc as stdalloc;`,
resolving to the single build-std `alloc`. Migrating to 0.17 + `default-hasher`
(foldhash-based) removed the conflict while preserving the `HashMap`/`HashMap::new()`
API the kernel uses. Verified green: `cargo build ... -Z build-std=core,alloc`
(T-00 gate) and `--features self_test` variant.

## Consequences
- O(1) average-case lookups in hot paths
- `foldhash` (via hashbrown `default-hasher`) provides DoS-resistant hashing; the
  `ahash` feature name was dropped in hashbrown 0.17 (see amendment above)
- hashbrown is no_std compatible with the `default-hasher` feature
- Remaining BTreeMap in PROCESS_TABLE is acceptable (low-update, low-QPS path)
- All kernel HashMaps now use hashbrown (`HashMap`/`HashMap::new()`, see Cargo.toml)