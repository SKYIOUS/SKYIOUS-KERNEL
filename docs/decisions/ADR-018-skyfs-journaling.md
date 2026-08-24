# ADR-018: SkyFS Journaling

## Status

**Proposed** — No team decision required; straightforward durability feature.

## Context

SkyFS is Vahi's native filesystem. Currently it has no journaling, meaning power loss or crash can corrupt metadata. Journaling provides crash recovery by logging changes before applying them.

Current state:
- SkyFS read/write works
- No journaling
- No crash recovery
- No fsync durability guarantees

## Decision

**Implement write-ahead logging (WAL) for SkyFS metadata.**

### Design

```text
Write path:
  1. Write change to journal (sequential, fast)
  2. Flush journal to disk
  3. Apply change to metadata (random, slow)
  4. Mark journal entry as committed

Recovery:
  1. Scan journal for uncommitted entries
  2. Replay uncommitted entries
  3. Mark entries as committed
```

### Acceptance Criteria

- [ ] Metadata changes logged before application
- [ ] Crash recovery replays uncommitted entries
- [ ] fsync durability semantics correct
- [ ] Atomic rename works
- [ ] No data loss on crash (metadata only)

## Consequences

### Positive

- Crash recovery for metadata
- fsync durability guarantees
- Atomic operations

### Negative

- Write overhead (journal writes)
- Space overhead (journal storage)
- Complexity (recovery logic)

## Alternatives Considered

### Alternative 1: No Journaling

**Rejected.** Data corruption on crash is unacceptable for production use.

### Alternative 2: Full Data Journaling

**Deferred.** Data journaling is slower. Start with metadata-only journaling.

### Alternative 3: Copy-on-Write (Btrfs-style)

**Deferred.** CoW is more complex. WAL is simpler and sufficient for SkyFS.
