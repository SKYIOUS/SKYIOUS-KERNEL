## Description

<!-- What does this change do and why is it needed? -->

## Checklist

- [ ] `cargo build` succeeds with 0 errors
- [ ] `cargo build --features self_test` succeeds (or known pre-existing failures documented)
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --exclude vahi_kernel --all-targets --all-features --locked -- -D warnings` passes
- [ ] `python scripts/check_repo_policy.py --diff-base origin/master` passes
- [ ] Commit messages follow Conventional Commits format
- [ ] All `unsafe` blocks have `// SAFETY:` justification
- [ ] No `unwrap()` in kernel code paths (non-test)
- [ ] Changes are limited to the intended area (no scope creep)
- [ ] Doc comments added for new public items

## Related Issues

<!-- Link to any issues this PR addresses -->

## Testing

<!-- How was this change tested? Describe the verification steps. -->