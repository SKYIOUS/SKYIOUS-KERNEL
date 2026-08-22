# Milestone Matrix

## Measurable Subsystem Status

Not letter grades — measurable criteria.

### Boot Reliability

| Milestone | Criteria | Status |
|-----------|----------|--------|
| Boot to kernel | Kernel starts, selftests pass | ✅ |
| Boot to shell | Userspace shell starts | ✅ |
| 100 consecutive boots | No failures in 100 QEMU boots | ❌ Not tested |

### Userspace Compatibility

| Milestone | Criteria | Status |
|-----------|----------|--------|
| Static binary | Hello world runs | ✅ |
| Shell | ash/dash starts, accepts commands | ❌ Not tested |
| Core utilities | ls, cat, echo, mkdir, rm work | ❌ Not tested |
| Dynamic linking | musl-linked binary runs | ❌ Not tested |
| nginx | nginx starts and serves | ❌ Not tested |
| redis | redis starts and responds | ❌ Not tested |

### VM Correctness

| Milestone | Criteria | Status |
|-----------|----------|--------|
| mmap | File mapping works | ✅ |
| COW | Fork uses COW correctly | ✅ |
| Page fault | Anonymous + file-backed faults work | ✅ |
| Memory pressure | OOM killer works | ❌ Not tested |

### VFS Correctness

| Milestone | Criteria | Status |
|-----------|----------|--------|
| Read/write | File I/O works | ✅ |
| Directory ops | mkdir/rmdir/readdir work | ✅ |
| Permissions | chmod/chown work | ✅ |
| Symlinks | symlink/readlink work | ✅ |

### Filesystem Correctness

| Milestone | Criteria | Status |
|-----------|----------|--------|
| SkyFS read/write | Basic file operations work | ✅ |
| SkyFS journaling | Crash recovery works | ✅ Fixed — replays data to target blocks |
| ext2 read | Read ext2 filesystem | ✅ |
| ext2 write | Write ext2 filesystem | ✅ |
| tmpfs | Mount and use tmpfs | ✅ Mounted at /tmp |

### SMP Correctness

| Milestone | Criteria | Status |
|-----------|----------|--------|
| 2-CPU boot | Boot with 2 CPUs | ✅ |
| 4-CPU boot | Boot with 4 CPUs | ⚠️ Needs verification |
| 8-CPU stress | 8-CPU stress test passes | ❌ Not tested |
| Scheduler correctness | No starvation, no deadlock | ✅ Stride + RT classes |

### Security

| Milestone | Criteria | Status |
|-----------|----------|--------|
| SMAP/SMEP | Enforced in page fault handler | ✅ CR4 flags + STAC/CLAC |
| KASLR | Kernel base randomized | ✅ init_kaslr() in main.rs |
| CFI | Control flow protected | ❌ Not implemented (P3) |
| Capabilities | Per-syscall checks work | ✅ |

### Networking

| Milestone | Criteria | Status |
|-----------|----------|--------|
| TCP | TCP connection works | ✅ |
| UDP | UDP send/receive works | ✅ |
| Unix sockets | Unix socket IPC works | ✅ |
| DHCP | DHCP assignment works | ✅ |
| DNS | DNS resolution works | ✅ |

### Performance

| Milestone | Criteria | Status |
|-----------|----------|--------|
| Fork throughput | 1000 forks/sec | ❌ Not measured |
| Pipe throughput | 100 MB/s | ❌ Not measured |
| TCP throughput | 1 Gbps | ❌ Not measured |
| Context switch | < 10μs | ❌ Not measured |

### Driver Coverage

| Milestone | Criteria | Status |
|-----------|----------|--------|
| Serial | Console I/O works | ✅ |
| Block (AHCI/NVMe/VirtIO) | Block I/O works | ✅ |
| Network (E1000/VirtIO) | Network I/O works | ✅ |
| USB (XHCI/UHCI) | USB device works | ✅ |
| GPU (VirtIO/BGA) | Display works | ✅ |

### Testing Coverage

| Milestone | Criteria | Status |
|-----------|----------|--------|
| Selftests | 92 tests pass | ✅ |
| Unit tests | #[test] functions pass | ⚠️ Needs verification |
| Integration | QEMU boot + userspace | ❌ Not implemented |
| Stress | SMP + memory + I/O stress | ❌ Not implemented |
| Fuzzing | 24-hour fuzz run | ❌ Not implemented |

## Quantitative Targets

Where measurable, define quantitative targets:

| Metric | Target | Measurement |
|--------|--------|-------------|
| Consecutive boots | 100 | QEMU boot test |
| Fork/exec iterations | 5000 | Stress test |
| Filesystem crash points | 1000 | Crash injection |
| 8-vCPU stress | 24 hours | Stress test |
| FD exhaustion | 10000 FDs | Stress test |
| fsync durability | 100 writes | Crash test |

**Note:** These targets are aspirational. Measure current state first, then set realistic targets.
