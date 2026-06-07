# Your First Contribution

Welcome to SkyOS! This guide helps new contributors make their first change to the kernel. Contributing to an operating system can feel daunting, but we've designed the process to be approachable.

## Getting Started

Start by setting up your development environment. Follow the instructions in `docs/build/BUILD.md` to install the required toolchain, including the Rust nightly compiler, QEMU, and the project's build dependencies. Once the build system is working, run the kernel in QEMU to verify your setup. Familiarize yourself with the repository layout by reading `docs/architecture/` for a high-level overview of the kernel's design.

## Good First Issues

We tag issues that are suitable for newcomers with the `good-first-issue` label. These issues are self-contained, have clearly defined scope, and often include implementation hints. Common first issues include:

- Adding missing error handling in existing code paths
- Implementing simple utility functions (e.g., string formatting helpers)
- Improving documentation and inline comments
- Writing or expanding unit tests
- Fixing minor bugs with clear reproduction steps

Browse the issue tracker and filter by `good-first-issue`. If an issue is unassigned, leave a comment expressing interest and a maintainer will follow up.

## Mentoring

New contributors can request a mentor by posting in the `#new-contributors` channel on Discord or tagging `@mentors` on their pull request. Mentors help with code reviews, explain kernel concepts, and guide you through the contribution process. We also hold periodic contributor onboarding sessions announced on Discord and the mailing list.

## Making Your First PR

1. Fork the repository and clone your fork.
2. Create a branch named after the issue or feature (e.g., `fix-1234` or `feat-pci-enum`).
3. Make your changes, following the coding standards in `coding_standards.md`.
4. Commit with a descriptive message following the Conventional Commits format.
5. Push your branch and open a pull request against the `main` branch.
6. Reference the issue number in the PR description (e.g., `Closes #1234`).
7. Wait for CI to pass and respond to reviewer feedback.

Don't worry if your first PR requires a few rounds of review -- that's normal. The goal is to learn and improve the codebase together.
