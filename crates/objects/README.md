# vahi-objects — Kernel Object Namespace & Handles

Unified kernel resource model: every open resource (files, sockets,
pipes, devices, processes) is a `KernelObject` accessed through
integer handles in per-process handle tables.

## Contents

- `handle.rs` — `HandleTable` and `HandleValue` types
- `namespace.rs` — Global object namespace tree
- `security.rs` — Security descriptors and access checks

## Dependencies

- `vahi-sync` — `IrqSafeMutex` for concurrent handle table access
- `alloc` — `Arc`, `Vec`, `String` for heap-allocated objects

## Invariants

- Refcounts are atomic (lock-free hot path)
- Handle table uses first-fit slot allocation
- Security checks on every handle insert
- Close-on-exec handled during fork
