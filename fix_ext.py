content = open('builder/build_limine_image.py').read()

# After compiling init, check for .exe extension
old_init = """        if ret.returncode != 0:
            print(f"  ERROR: Failed to compile init: {ret.stderr}")
            return b''
        print(f"  Created {init_bin}")"""
new_init = """        if ret.returncode != 0:
            print(f"  ERROR: Failed to compile init: {ret.stderr}")
            return b''
        # On Windows, gcc may produce .exe
        if not os.path.exists(init_bin) and os.path.exists(init_bin + '.exe'):
            init_bin = init_bin + '.exe'
        print(f"  Created {init_bin}")"""
assert old_init in content, 'old_init not found'
content = content.replace(old_init, new_init)

# Same for sash
old_sash = """        if ret.returncode != 0:
            print(f"  ERROR: Failed to compile sash: {ret.stderr}")
            return b''
        print(f"  Created {sash_bin}")"""
new_sash = """        if ret.returncode != 0:
            print(f"  ERROR: Failed to compile sash: {ret.stderr}")
            return b''
        # On Windows, gcc may produce .exe
        if not os.path.exists(sash_bin) and os.path.exists(sash_bin + '.exe'):
            sash_bin = sash_bin + '.exe'
        print(f"  Created {sash_bin}")"""
assert old_sash in content, 'old_sash not found'
content = content.replace(old_sash, new_sash)

# Also for vdso_libc.a - it should be fine as-is

open('builder/build_limine_image.py', 'w').write(content)
print('done')
