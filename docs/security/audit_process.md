# Security Audit Process

SkyOS undergoes regular security audits to identify and remediate vulnerabilities. This document describes the audit process and checklist used by auditors.

## Audit Scope

Security audits cover the kernel core, memory manager, scheduler, VFS, system call interface, and critical drivers. Each audit targets specific subsystems based on recent changes, known vulnerability patterns, and the current maturity of the code. The scope is defined at the start of each audit cycle and documented in the audit report.

## Audit Phases

An audit proceeds through four phases:

1. **Planning**: The audit lead defines scope, assembles the audit team, and gathers relevant documentation and source code. Threat models and attack surfaces are identified for each subsystem under review.
2. **Static Analysis**: The team reviews source code manually and with automated tools. Focus areas include `unsafe` block correctness, locking discipline, integer overflow handling, and input validation at kernel-user boundaries.
3. **Dynamic Testing**: The kernel is exercised under fuzz testing and stress conditions. Custom test harnesses probe system calls with malformed inputs, race conditions are induced, and resource exhaustion scenarios are tested.
4. **Reporting**: Findings are documented with severity ratings, reproduction steps, and remediation recommendations. Critical and high-severity findings are addressed immediately; medium and low findings are triaged into the issue tracker.

## Audit Checklist

Auditors verify the following for each subsystem:

- All `unsafe` blocks are correct and accompanied by `// SAFETY:` comments.
- User-supplied pointers are validated before dereferencing.
- Integer arithmetic is checked for overflow (using `checked_*` or `saturating_*` operations).
- Lock ordering follows the established hierarchy and there are no deadlock scenarios.
- Capability checks are applied consistently at every security boundary.
- Interrupt safety is maintained in code paths shared between normal and interrupt context.
- Error paths do not leak resources or leave data structures in inconsistent states.

## Remediation Timeline

Critical vulnerabilities must be fixed within 7 days of confirmation. High-severity issues have a 30-day window. Medium and low findings are addressed in the next planned release cycle. All fixes are reviewed by at least two maintainers before merging.
