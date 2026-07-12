"""
Build a .sky package file (ustar tar archive containing a `manifest` + files).

Usage:
    python scripts/make_sky_pkg.py pkg_dir output.skp

The pkg_dir must contain a `manifest` file with:
    name=package-name
    version=1.0.0
    description=...
    depends=pkg1,pkg2  (optional)

All other files in pkg_dir are included as package payload.
"""

import os
import sys
import struct
import hashlib

BLOCK_SIZE = 512


def make_ustar_header(path: bytes, size: int, filetype: str = "0") -> bytes:
    """Create a 512-byte ustar header for a file."""
    name = path[:100].ljust(100, b"\0")
    mode = b"0000644\0"
    uid = b"0000000\0"
    gid = b"0000000\0"
    size_bytes = f"{size:011o}".encode() + b"\0"  # 12 bytes octal
    mtime = b"00000000000\0"
    checksum = b" " * 8
    typeflag = filetype.encode()
    linkname = b"\0" * 100
    magic = b"ustar\0"
    version = b"00"
    uname = b"root\0" + b"\0" * 27
    gname = b"root\0" + b"\0" * 27
    devmajor = b"\0" * 8
    devminor = b"\0" * 8
    prefix = b"\0" * 155
    pad = b"\0" * 12

    data = (
        name + mode + uid + gid + size_bytes + mtime + checksum
        + typeflag + linkname + magic + version + uname + gname
        + devmajor + devminor + prefix + pad
    )
    assert len(data) == BLOCK_SIZE, f"header len={len(data)}"

    # Compute checksum (sum of bytes, with checksum field treated as spaces)
    raw = bytearray(data)
    for i in range(148, 156):
        raw[i] = ord(" ")
    cksum = sum(raw) & 0xFFFFFFFF
    cksum_str = f"{cksum:06o}\0 ".encode()
    data = data[:148] + cksum_str + data[156:]
    return data


def pad_block(data: bytes) -> bytes:
    """Pad data to a multiple of BLOCK_SIZE with null bytes."""
    if len(data) % BLOCK_SIZE == 0:
        return data
    return data + b"\0" * (BLOCK_SIZE - len(data) % BLOCK_SIZE)


def build_pkg(pkg_dir: str, output: str):
    manifest_path = os.path.join(pkg_dir, "manifest")
    if not os.path.exists(manifest_path):
        print(f"Error: {pkg_dir}/manifest not found", file=sys.stderr)
        sys.exit(1)

    with open(manifest_path, "rb") as f:
        manifest_data = f.read()

    # Collect all files (including manifest)
    entries = {}
    for root, dirs, files in os.walk(pkg_dir):
        for fname in files:
            fpath = os.path.join(root, fname)
            relpath = os.path.relpath(fpath, pkg_dir).replace("\\", "/")
            with open(fpath, "rb") as f:
                entries[relpath] = f.read()

    # Build tar archive
    archive = bytearray()
    for name in sorted(entries.keys()):
        data = entries[name]
        header = make_ustar_header(name.encode(), len(data))
        archive.extend(header)
        archive.extend(pad_block(data))

    # End-of-archive markers (two zero blocks)
    archive.extend(b"\0" * BLOCK_SIZE * 2)

    with open(output, "wb") as f:
        f.write(archive)

    sha256 = hashlib.sha256(archive).hexdigest()
    print(f"Created {output}: {len(archive)} bytes ({len(entries)} files)")
    print(f"SHA256: {sha256}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <pkg_dir> <output.skp>", file=sys.stderr)
        sys.exit(1)
    build_pkg(sys.argv[1], sys.argv[2])
