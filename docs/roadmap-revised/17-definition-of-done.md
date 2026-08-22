# Definition of Done

## For Every Change

- [ ] All existing tests pass without modification
- [ ] Build succeeds with no new warnings
- [ ] Clippy passes with no new warnings
- [ ] Code follows project conventions
- [ ] No error handling removed or weakened
- [ ] No dead code left behind
- [ ] Diff is clean and reviewable
- [ ] Behavior is preserved (no unintended changes)

## For Every Phase

- [ ] All acceptance criteria met
- [ ] All verification steps pass
- [ ] No regressions from previous phases
- [ ] Documentation updated
- [ ] ADRs written for architectural decisions
- [ ] Technical debt paid down (not increased)

## For the Kernel as a Whole

### Boot Reliability
- [ ] 100 consecutive QEMU boots without failure
- [ ] Boot with various feature flag combinations
- [ ] Boot with SMP (1, 2, 4 CPUs)

### Userspace Compatibility
- [ ] Static musl binaries compile and run
- [ ] Shell starts and accepts commands
- [ ] Core utilities work (ls, cat, echo, mkdir, rm)
- [ ] Dynamic linking works (if implemented)
- [ ] nginx runs (or specific failure identified)
- [ ] redis runs (or specific failure identified)

### VM Correctness
- [ ] mmap works for file-backed and anonymous mappings
- [ ] COW works correctly across fork
- [ ] Page fault handler works for all fault types
- [x] Memory pressure triggers OOM killer

### VFS Correctness
- [ ] File read/write works
- [ ] Directory operations work
- [ ] Permissions work
- [ ] Symlinks work

### Filesystem Correctness
- [ ] SkyFS read/write works
- [ ] SkyFS journaling works
- [ ] Crash recovery works
- [ ] ext4 read works
- [ ] ext4 write works (if implemented)
- [ ] tmpfs works

### SMP Correctness
- [ ] 2-CPU boot works
- [ ] 4-CPU boot works
- [ ] 8-CPU stress test passes
- [ ] No scheduler starvation
- [ ] No deadlocks

### Security
- [ ] SMAP/SMEP enforced
- [ ] KASLR active
- [ ] CFI protects control flow
- [ ] Capabilities work
- [ ] Landlock/sandboxing works

### Networking
- [ ] TCP works
- [ ] UDP works
- [ ] Unix sockets work
- [ ] DHCP works
- [ ] DNS works

### Performance
- [ ] Fork throughput measured
- [ ] Pipe throughput measured
- [ ] TCP throughput measured
- [ ] Context switch time measured

### Testing
- [ ] All selftests pass
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Stress tests pass
- [ ] Fuzzing finds no crashes for 24 hours

### CI/CD
- [ ] PR smoke runs on every PR
- [ ] Integration runs on every merge
- [ ] Extended runs nightly
- [ ] Nightly runs weekly
- [ ] All artifacts uploaded
