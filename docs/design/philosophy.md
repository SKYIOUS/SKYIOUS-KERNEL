# Design Philosophy and Goals

SkyOS is built on a set of core design principles that guide every architectural decision.

## Core Principles

### 1. Safety First

Memory safety is the #1 priority. By using Rust as the implementation language, we eliminate buffer overflows, use-after-free, and many other classes of vulnerabilities at compile time. Unsafe code is minimized, reviewed extensively, and documented with safety invariants.

### 2. Asynchronous by Default

The kernel is designed around async/await from the ground up. System calls, driver operations, and IPC are all non-blocking. This provides deterministic interleaving, reduced context switch overhead, and simpler reasoning about concurrency compared to preemptive threading.

### 3. Minimalism

The kernel core is intentionally small. Filesystem implementations, network stacks, and device drivers run as userspace tasks. The kernel provides only the essentials: scheduling, memory management, IPC primitives, and security isolation.

### 4. Modularity through Capabilities

Rather than a traditional user/root security model, SkyOS uses capability-based security. Each task holds capabilities granting access to specific resources. Capabilities can be delegated, revoked, and scoped.

### 5. Performance without Compromise

Safety and abstraction should not come at the cost of performance. The async model, work-stealing scheduler, and careful zero-cost abstraction usage ensure that SkyOS remains competitive with traditional C-based kernels.

## Design Goals

- **Correctness**: Provable memory safety and race-free execution
- **Performance**: Competitive with Linux for common workloads
- **Simplicity**: Clean, well-documented code that is easy to understand
- **Extensibility**: Easy to add new drivers, filesystems, and syscalls
- **Security**: Capability-based isolation with minimal trusted computing base
