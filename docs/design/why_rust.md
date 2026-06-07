# Why Rust Was Chosen for Kernel Development

The decision to implement SkyOS in Rust was driven by concrete technical advantages over C and C++.

## Memory Safety Without Garbage Collection

Rust's ownership model and borrow checker provide compile-time memory safety guarantees without a garbage collector or reference counting overhead. In kernel development, this eliminates:

- Buffer overflows (the most common kernel vulnerability)
- Use-after-free bugs
- Double-free errors
- Dangling pointer dereferences

These classes of bugs account for approximately 65% of all critical CVEs in the Linux kernel.

## Zero-Cost Abstractions

Rust's abstractions compile to the same efficient machine code as hand-written C. Features like closures, iterators, and generics have no runtime overhead. The async/await mechanism in particular compiles to efficient state machines.

## Fearless Concurrency

Rust's type system enforces thread safety at compile time through the `Send` and `Sync` traits. The kernel can safely share data between cores without runtime lock checking, reducing the risk of data races.

## Unsafe Code Discipline

While kernel development requires some unsafe code for hardware access, Rust's `unsafe` keyword makes these sections explicit and auditable. Linux-style bugs that span 50 lines of implicit unsafe C are replaced by small, reviewed unsafe blocks.

## The Cost

The primary tradeoff is the learning curve and compile-time checking strictness. Some patterns that are easy in C (intrusive linked lists, explicit memory layout) require more thought in Rust. However, the long-term maintenance benefits far outweigh the initial development friction.
