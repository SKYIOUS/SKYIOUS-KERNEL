# Maintainers and Their Areas

This page lists the current SkyOS maintainers and their areas of expertise.

## Core Maintainers

| Name | GitHub | Area |
|------|--------|------|
| Alice Developer | @alicedev | Kernel core, memory management |
| Bob Engineer | @bobeng | Scheduler, async executor |
| Carol Contributor | @carolc | Drivers, PCI, ACPI |

## Module Maintainers

### Architecture
- **x86_64**: Alice Developer (@alicedev)
- **aarch64** (future): *Open position*

### Memory Management
- **Page allocator**: Alice Developer (@alicedev)
- **Virtual memory**: David Dev (@davidd)

### Scheduler and IPC
- **Async executor**: Bob Engineer (@bobeng)
- **IPC framework**: Carol Contributor (@carolc)

### Drivers
- **PS/2, keyboard, mouse**: Carol Contributor (@carolc)
- **Framebuffer**: Ellen Eng (@elleneng)
- **Network (e1000, VirtIO)**: Frank Fellow (@frankf)
- **Storage (AHCI, NVMe)**: *Open position*

### Filesystem
- **VFS layer**: David Dev (@davidd)
- **ext2 support**: *Open position*

### Build and CI
- **Build system**: Bob Engineer (@bobeng)
- **CI/CD**: Alice Developer (@alicedev)

## Becoming a Maintainer

To become a maintainer:
1. Contribute consistently to the project over several months
2. Demonstrate expertise in your area
3. Be nominated by an existing maintainer
4. Approved by consensus of the core maintainers

## Maintainer Responsibilities

- Review PRs in your area within 3 business days
- Triage issues related to your area
- Mentor new contributors
- Participate in design discussions
- Help maintain documentation
