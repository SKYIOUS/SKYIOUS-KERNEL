# Changelog Guidelines

Every pull request that modifies kernel behavior, fixes a bug, or adds a feature must include a changelog entry. This document explains how to write and format entries.

## Location and Format

Changelog entries are stored in `docs/CHANGELOG.md`. The file follows the [Keep a Changelog](https://keepachangelog.com/) format with version sections. Each release groups changes under the following categories: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, and `Security`. Entries within each category are listed as bullet points in present tense.

## Writing an Entry

Each changelog entry should be a concise, user-facing description of the change. Use the following structure:

```
- {Brief description of the change} (#{PR number})
```

For example:

```
- Added support for PCI MSI-X interrupt vectors (#892)
- Fixed race condition in the VFS dentry cache (#901)
- Changed scheduler timeslice from 10ms to 5ms (#908)
```

The description should explain what changed from the perspective of someone reading the changelog, not the implementation details. Avoid internal jargon and focus on observable behavior.

## When to Add an Entry

Add a changelog entry for any change that affects:

- System call behavior or ABI
- Kernel configuration and boot options
- Hardware support and drivers
- Performance characteristics
- Bug fixes that users or driver authors would notice
- New APIs or modified existing APIs

Internal refactoring, documentation-only changes, and test additions generally do not need changelog entries unless they have a visible impact.

## Including the Entry in a PR

Add your changelog entry in the same pull request as the code change. Place it under the `## Unreleased` section at the top of `docs/CHANGELOG.md`. If the section does not yet exist, create it. During the merge, a maintainer will move the entries into the appropriate release version section.
