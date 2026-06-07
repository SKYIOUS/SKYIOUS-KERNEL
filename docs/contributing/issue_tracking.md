# How to Report Bugs and Request Features

This page explains how to use the SkyOS issue tracker.

## Bug Reports

When reporting a bug, please include:

### Required Information
- **Description**: Clear, concise description of the bug
- **Environment**: Host OS, QEMU version, Rust toolchain version
- **Reproduction steps**: Exact steps to reproduce the issue
- **Expected behavior**: What should happen
- **Actual behavior**: What actually happens
- **Logs**: Relevant kernel logs or serial output

### Bug Report Template

```markdown
## Bug Description
[Clear description of the bug]

## Environment
- Host OS: [e.g., Ubuntu 22.04]
- QEMU version: [e.g., 7.2.0]
- Rust toolchain: [e.g., nightly-2024-01-01]

## Reproduction Steps
1. [Step 1]
2. [Step 2]
3. [Step 3]

## Expected Behavior
[What should happen]

## Actual Behavior
[What actually happens]

## Logs
```
[Relevant logs]
```
```

## Feature Requests

When requesting a feature, please describe:
- **Use case**: What problem does this feature solve?
- **Proposed solution**: How should the feature work?
- **Alternatives**: Any alternative solutions considered?
- **Priority**: How important is this feature to you?

## Issue Labels

| Label | Description |
|-------|-------------|
| `bug` | Confirmed bug |
| `feature` | Feature request |
| `good-first-issue` | Good for new contributors |
| `help-wanted` | Need assistance |
| `discussion` | Open for debate |
| `priority-high` | Should be addressed soon |
| `priority-low` | Nice to have |

## Issue Lifecycle

1. Issue is opened and automatically labeled
2. Maintainer reviews and adds appropriate labels
3. Issue is assigned to a milestone
4. Work begins (linked to a PR)
5. PR merges, issue is closed with a reference to the fix
