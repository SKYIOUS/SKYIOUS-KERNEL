# Pull Request Process

This page describes the process for submitting pull requests to the SkyOS project.

## Before Submitting

1. Ensure your code compiles: `cargo check --release`
2. Run the test suite: `cargo test --lib`
3. Run the linter: `cargo clippy --all-targets`
4. Run integration tests: `cargo test --test integration`
5. Review your changes with `git diff` to ensure no unintended changes

## PR Guidelines

- **One PR per feature/bugfix**: Keep changes focused and reviewable
- **Write meaningful commit messages**: Follow conventional commits format
- **Include tests**: New features should include tests; bug fixes should include regression tests
- **Update documentation**: API changes must be reflected in documentation
- **Keep PRs small**: Large PRs are hard to review; split them if possible

## PR Template

```markdown
## Summary
Brief description of the changes.

## Related Issues
Fixes #123, Closes #456

## Testing
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Manual testing in QEMU

## Checklist
- [ ] Code follows project style
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] No new compiler warnings
```

## Review Process

1. A maintainer will review your PR within 1-3 business days
2. Address all review comments
3. Once approved, a maintainer will merge your PR
4. The PR will be included in the next release

## After Merge

- Delete your feature branch
- Check that CI passes on the main branch
- Celebrate your contribution!
