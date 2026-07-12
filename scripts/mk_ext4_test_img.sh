#!/bin/bash
# Generate an ext4 test image for the Vahi kernel ext4 driver self-test.
# Usage: ./mk_ext4_test_img.sh [output_path] [size_mb]

OUT=${1:-ext4_test.img}
SIZE=${2:-32}

rm -f "$OUT"
dd if=/dev/zero of="$OUT" bs=1M count=$SIZE 2>/dev/null

# Create ext4 without features we don't support
# Enable: extents, flex_bg (we handle), dir_nlink, extra_isize, sparse_super, large_file
# Disable: has_journal, 64bit, metadata_csum, huge_file, mmp, quota, bigalloc, inline_data, encrypt
mkfs.ext4 -F -q \
  -O extent,flex_bg,dir_nlink,extra_isize,sparse_super,large_file \
  -O ^has_journal,^64bit,^metadata_csum,^huge_file,^mmp,^quota,^bigalloc,^inline_data,^encrypt,^orphan_file \
  -E lazy_itable_init=0,lazy_journal_init=0 \
  "$OUT" 2>/dev/null || {
  echo "mkfs.ext4 failed, trying simpler options..."
  mkfs.ext4 -F -q -O extent,^has_journal,^metadata_csum,^64bit "$OUT" 2>/dev/null
}

# Mount and populate
MNT=$(mktemp -d)
sudo mount -o loop "$OUT" "$MNT" 2>/dev/null || {
  echo "mount failed, trying without loop..."
  sudo mount "$OUT" "$MNT" 2>/dev/null
}

# Create test files
echo "Hello from ext4!" | sudo tee "$MNT/hello.txt" >/dev/null
echo "Ext4 filesystem driver test" | sudo tee "$MNT/README" >/dev/null

# Create directory structure
sudo mkdir -p "$MNT/test/sub"
echo "nested file content" | sudo tee "$MNT/test/sub/deep.txt" >/dev/null
echo "another test" | sudo tee "$MNT/test/file.txt" >/dev/null

# Create a larger file for read benchmarking
dd if=/dev/urandom bs=1024 count=64 2>/dev/null | base64 | sudo tee "$MNT/test/large.txt" >/dev/null

# Create symlink
ln -sf "hello.txt" "$MNT/test/link_to_hello" 2>/dev/null

sync
sudo umount "$MNT" 2>/dev/null
rmdir "$MNT"

echo "Created $OUT ($SIZE MB) with ext4 test filesystem"
