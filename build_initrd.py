"""Build SkyOS initrd.tar with FHS directory structure."""
import tarfile, os, sys, io

def build_initrd(root_dir: str, output_path: str):
    coreutils_bins = [
        'ls', 'cat', 'mkdir', 'rm', 'cp', 'mv',
        'ps', 'clear', 'uname', 'printenv', 'sleep', 'yes',
        'rmdir', 'touch', 'hostname', 'which', 'env', 'echo', 'head', 'tail', 'wc', 'grep', 'ln', 'chmod',
        'printf', 'sort', 'uniq', 'uptime',
        'ping', 'nslookup', 'wget', 'ifconfig', 'netstat', 'telnet',
        'beep', 'dd', 'blkid', 'fdisk', 'df', 'du',
    ]

    binaries = {
        'bin/init':        'init',
        'bin/sargash':     'sargash',
        'bin/svc':         'svc',
        'bin/vahid':       'vahid',
        'bin/skyedit':     'skyedit',
        'bin/sarga-disp':  'sarga-disp',
        'bin/skypkg':     'skypkg',
        'bin/login':      'login',
        'bin/passwd':     'passwd',
        'bin/skybuild':   'skybuild',
        'bin/setup':      'setup',
        'bin/mkfs.ext2':   'mkfs_ext2',
        'bin/mkfs.fat':    'mkfs_fat',
    }
    for b in coreutils_bins:
        binaries[f'bin/{b}'] = b

    symlinks = {
        'sbin/init':    '../bin/init',
        'sbin/vahid':   '../bin/vahid',
        'sbin/svc':     '../bin/svc',
        'sbin/skyedit': '../bin/skyedit',
        'sbin/sarga-disp': '../bin/sarga-disp',
    }

    empty_dirs = [
        'dev',
        'proc',
        'tmp',
        'usr/lib',
        'usr/share',
        'usr/include',
        'var/log',
        'var/cache',
        'var/spool',
        'var/skypkg',
        'home/root',
        'mnt/cdrom',
        'mnt/usb',
    ]

    config_files = {
        'etc/init.cfg': None,
        'etc/fstab': None,
        'etc/hostname': None,
        'etc/passwd': None,
        'etc/shadow': None,
        'etc/group': None,
    }

    if os.path.exists(output_path):
        os.remove(output_path)

    with tarfile.open(output_path, 'w') as tar:
        # Add regular binaries
        for arcname, binary in binaries.items():
            full_path = os.path.join(root_dir, 'bin', binary)
            if os.path.exists(full_path):
                tar.add(full_path, arcname=arcname)
                print(f'  {arcname} ({os.path.getsize(full_path)} bytes)')
            else:
                print(f'  WARNING: {binary} not found at {full_path}')

        # Add config files
        config_data = {
            'etc/init.cfg': read_config(root_dir, 'etc/init.cfg'),
            'etc/fstab': FSTAB_CONTENT,
            'etc/hostname': HOSTNAME_CONTENT,
        }
        for arcname, data in config_data.items():
            info = tarfile.TarInfo(name=arcname)
            info.type = tarfile.REGTYPE
            encoded = data.encode('utf-8')
            info.size = len(encoded)
            tar.addfile(info, io.BytesIO(encoded))
            print(f'  {arcname} ({len(encoded)} bytes)')

        # Add symlinks
        for arcname, target in symlinks.items():
            info = tarfile.TarInfo(name=arcname)
            info.type = tarfile.SYMTYPE
            info.linkname = target
            tar.addfile(info)
            print(f'  {arcname} -> {target}')

        # Add empty directories
        for dirname in empty_dirs:
            info = tarfile.TarInfo(name=dirname)
            info.type = tarfile.DIRTYPE
            info.mode = 0o755
            tar.addfile(info)
            print(f'  {dirname}/')

    size = os.path.getsize(output_path)
    print(f'\ninitrd.tar: {size} bytes ({size/1024:.1f} KB)')

def read_config(root_dir, path):
    full = os.path.join(root_dir, path)
    if os.path.exists(full):
        with open(full, 'r') as f:
            return f.read()
    return ''

FSTAB_CONTENT = """# /etc/fstab - filesystem mount table
# <source>  <mountpoint>  <fstype>  <options>  <dump>  <pass>
tmpfs       /tmp          tmpfs     defaults   0       0
tmpfs       /var/log      tmpfs     defaults   0       0
tmpfs       /var/cache    tmpfs     defaults   0       0
tmpfs       /home         tmpfs     defaults   0       0
"""

HOSTNAME_CONTENT = "skyos\n"

if __name__ == '__main__':
    root = sys.argv[1] if len(sys.argv) > 1 else '.'
    output = os.path.join(root, 'initrd.tar')
    build_initrd(root, output)
