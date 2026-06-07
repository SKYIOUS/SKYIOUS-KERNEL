# Code Review Process

All changes to SkyOS must go through the code review process. This ensures code quality, consistency, and security across the kernel.

## Before Submitting

Before opening a pull request, ensure the following checklist is complete:

1. The code builds without warnings (`cargo build --all-targets`).
2. All existing tests pass (`cargo test`).
3. New code includes appropriate tests (unit, integration, or both).
4. The code has been formatted with `rustfmt`.
5. No new `unsafe` blocks are introduced without a `// SAFETY:` comment.
6. Public API surfaces have documentation comments.
7. The commit message follows the Conventional Commits format.
8. The branch is based on the latest `main` and contains no merge conflicts.

## Review Lifecycle

A pull request goes through several stages:

1. **Draft**: Author opens a draft PR for early feedback. CI runs but merging is blocked.
2. **Ready for Review**: Author marks the PR as ready. At least two maintainers must approve.
3. **Review Cycle**: Reviewers leave comments. The author addresses feedback with additional commits. Once all conversations are resolved, reviewers approve.
4. **Final Review**: A maintainer with merge privileges performs a final pass, verifying the checklist above.
5. **Merge**: The PR is squashed-merged into `main` with a clean commit message.

## What Reviewers Look For

Reviewers evaluate each PR on:

- **Correctness**: Does the code do what it claims? Are edge cases handled?
- **Safety**: Are `unsafe` blocks justified and correct? Could the change introduce memory unsafety?
- **Performance**: Are there obvious performance regressions? Are allocations minimized in hot paths?
- **Style**: Does the code follow the project's coding standards? Is it readable?
- **Test coverage**: Are there tests for new functionality? Do they cover error paths?

## Responding to Feedback

When a reviewer requests changes, address each comment in a follow-up commit. Do not force-push or rebase during review, as it makes it harder to track what changed. Once all feedback is addressed, re-request review rather than leaving the PR idle. Be respectful and open to suggestions; the review process is collaborative.

## Merge Criteria

A PR may be merged when it has at least two approvals from maintainers, all CI checks pass, and there are no unresolved conversations. Maintainers reserve the right to reject PRs that do not meet the project's quality bar.
