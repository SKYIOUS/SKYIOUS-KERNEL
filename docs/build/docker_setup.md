# Using Docker for Build Environment

Docker provides a reproducible build environment for SkyOS.

## Docker Image

A pre-built Docker image with all dependencies is available:

```bash
docker pull skyos/build:latest
```

## Building with Docker

```bash
# Build the kernel inside Docker
docker run --rm -v $(pwd):/skyos -w /skyos skyos/build:latest cargo build --release

# Build and run in QEMU (requires privileged mode)
docker run --rm --privileged -v $(pwd):/skyos -w /skyos skyos/build:latest cargo run --release
```

## Docker Compose

```yaml
# docker-compose.yml
services:
  skyos-build:
    image: skyos/build:latest
    volumes:
      - .:/skyos
    working_dir: /skyos
    command: cargo build --release
```

## Custom Dockerfile

```dockerfile
FROM ubuntu:22.04

RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    qemu-system-x86 \
    python3 \
    xorriso \
    make

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup install nightly && \
    rustup target add x86_64-unknown-none --toolchain nightly && \
    rustup component add rust-src --toolchain nightly && \
    rustup component add llvm-tools-preview --toolchain nightly

WORKDIR /skyos
```

## Benefits

- Identical build environment across all developers
- No dependency conflicts with host system
- Easy CI/CD integration
- Reproducible builds for release verification
