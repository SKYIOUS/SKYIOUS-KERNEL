#!/bin/sh
# Create a 16 MB ext2 image with test files for the ext2 driver.
# Requires: dd, mkfs.ext2, mount (as root), or debugfs for non-root.

set -e
IMG=${1:-test.img}
MNT=$(mktemp -d)

# 16 MB image
dd if=/dev/zero of=$IMG bs=1M count=16 2>/dev/null

# Create ext2 filesystem (no journal, no huge files)
mkfs.ext2 -q -F -b 1024 -N 512 $IMG

# If we can mount (root), add test files; otherwise use debugfs
if [ "$(id -u)" = "0" ]; then
    mount -o loop $IMG $MNT
    echo "Hello from ext2!" > $MNT/hello.txt
    mkdir -p $MNT/subdir
    echo "nested" > $MNT/subdir/nested.txt
    ln -s hello.txt $MNT/link.txt
    umount $MNT
else
    debugfs -w -R 'write /dev/stdin hello.txt' $IMG <<EOF
Hello from ext2!
EOF
    debugfs -w -R 'mkdir subdir' $IMG
    debugfs -w -R 'write /dev/stdin subdir/nested.txt' $IMG <<EOF
nested
EOF
    debugfs -w -R 'link hello.txt link.txt' $IMG 2>/dev/null || true
fi

rmdir $MNT
echo "Created $IMG"
