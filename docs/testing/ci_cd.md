# CI/CD Pipeline

SkyOS uses continuous integration and deployment for quality assurance.

## CI Provider

The project uses GitHub Actions for CI/CD. The pipeline runs on every push to any branch and on every pull request.

## Pipeline Stages

```
1. Lint (cargo check + clippy)
2. Build (debug + release)
3. Unit tests (cargo test --lib)
4. Integration tests (QEMU)
5. Regression tests
6. Coverage report
7. Documentation generation
8. Artifact publishing
```

## CI Configuration

```yaml
# .github/workflows/ci.yml
name: SkyOS CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: nightly
          target: x86_64-unknown-none
      - run: cargo check
      - run: cargo test --lib
      - run: cargo test --test integration
```

## Build Artifacts

Successful builds produce:
- Kernel binary (`skyos`)
- Bootable image (`bootimage-skyos.bin`)
- Documentation (HTML)
- Coverage report (HTML)
- Debug symbols

Artifacts are stored for 30 days and linked from the build log.

## Release Process

1. Version bump following semver
2. Changelog update
3. Tagged release on GitHub
4. Automated binary publication
5. Documentation deployment to project website

## Testing Matrix

| Configuration | Toolchain | Build | Tests |
|--------------|-----------|-------|-------|
| Debug | nightly | cargo build | Unit + integration |
| Release | nightly | cargo build --release | Unit + integration |
| KASAN | nightly | cargo build --features kasan | Integration |
| SMP=4 | nightly | cargo build --release | SMP integration |
