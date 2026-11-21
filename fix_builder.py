content = open('builder/build_limine_image.py').read()

# Fix 1: Add -nostartfiles to init compilation
old_init = """        ret = subprocess.run([
            compiler, '-static', '-Os', '-Wall', '-Wextra',
            '-I', userspace_dir,
            init_c, vdso_libc_a, '-o', init_bin
        ], capture_output=True, text=True)"""
new_init = """        ret = subprocess.run([
            compiler, '-static', '-nostartfiles', '-Os', '-Wall', '-Wextra',
            '-I', userspace_dir,
            init_c, vdso_libc_a, '-o', init_bin
        ], capture_output=True, text=True)"""
assert old_init in content, 'old_init not found'
content = content.replace(old_init, new_init)

# Fix 2: Add -nostartfiles to sash compilation
old_sash = """        ret = subprocess.run([
            compiler, '-static', '-Os', '-Wall', '-Wextra',
            '-I', userspace_dir,
            sash_c, vdso_libc_a, '-o', sash_bin
        ], capture_output=True, text=True)"""
new_sash = """        ret = subprocess.run([
            compiler, '-static', '-nostartfiles', '-Os', '-Wall', '-Wextra',
            '-I', userspace_dir,
            sash_c, vdso_libc_a, '-o', sash_bin
        ], capture_output=True, text=True)"""
assert old_sash in content, 'old_sash not found'
content = content.replace(old_sash, new_sash)

open('builder/build_limine_image.py', 'w').write(content)
print('done')
