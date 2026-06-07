# How to Contribute

We welcome contributions to SkyOS. This guide outlines the contribution process.

## Getting Started

1. Fork the repository on GitHub
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/skyos.git`
3. Set up the build environment (see [Getting Started](getting_started.md))
4. Find an issue labeled `good-first-issue` or `help-wanted`

## Making Changes

- Create a feature branch: `git checkout -b my-feature`
- Write code following the [coding style](coding_style.md)
- Add or update tests as appropriate
- Run the test suite: `cargo test`
- Ensure all existing tests pass

## Commit Messages

Follow conventional commits format:
```
feat: add virtio-blk driver support
fix: correct TLB flush on page table update
docs: update interrupt handling documentation
```

## Submitting a Pull Request

1. Push your branch to GitHub
2. Open a pull request against `main`
3. Fill out the PR template with:
   - Summary of changes
   - Related issues
   - Testing performed
   - Screenshots (if UI changes)
4. Wait for CI to pass
5. Request review from a maintainer

## Code Review

All submissions require review. Reviewers will check:
- Correctness and safety
- Code style and conventions
- Test coverage
- Performance implications
