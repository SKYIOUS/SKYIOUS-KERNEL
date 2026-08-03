# ADR-006: POSIX Capabilities + LSM for Security Model

## Status
Accepted

## Date
2026-08-01

## Context
The kernel needed a security architecture that allows controlled privilege escalation without full root. Requirements:
- Fine-grained privilege separation (not just root vs user)
- POSIX UID/GID compatibility for Linux emulation
- Auditable security decisions
- Extensible to Mandatory Access Control (MAC)
- No performance overhead on the fast path when no MAC policy is loaded

## Decision
Use a layered security model:
1. **DAC**: POSIX UID/GID file permissions (owner/group/other, rwx bits)
2. **Capabilities**: Bitmask-based privilege checks (Linux-compatible positions)
3. **LSM**: Rule-based MAC loaded from `/etc/lsm_policy`

Key capabilities implemented:
| Capability | Bit | Guarded syscalls |
|---|---|---|
| CAP_SYS_ADMIN | 21 | mount(), privileged operations |
| CAP_KILL | 5 | kill() across UIDs |
| CAP_SYS_BOOT | 22 | reboot() |
| CAP_SETUID | 6 | setuid() to any UID |
| CAP_SETGID | 7 | setgid() to any GID |
| CAP_NET_RAW | 13 | socket(SOCK_RAW) |
| CAP_DAC_OVERRIDE | 1 | Bypass file permission checks |

All capability checks are audited to serial output with PID context.

## Alternatives Considered

### Pure DAC (no capabilities)
- Pros: Simple, matches traditional Unix
- Cons: All-or-nothing root privilege, many syscalls need specific privilege
- Rejected: Too coarse for the feature set

### Seccomp-only
- Pros: Fine-grained syscall filtering, proven in Linux
- Cons: No file/DAC integration, harder to configure
- Rejected: Supplements, doesn't replace DAC

### Full MAC mandatory (e.g., SELinux-style)
- Pros: Strongest security model
- Cons: Complex policy language, high maintenance burden
- Rejected: LSM skeleton provides optional MAC without mandatory overhead

## Consequences
- Linux emulation can map Linux capability checks directly
- Capability checks are O(1) bitmask operations — no measurable overhead
- LSM is a no-op until `/etc/lsm_policy` is created
- Security decisions logged for audit trail
- Syscall filter per process (bitmask) provides additional isolation