import tarfile, re

with tarfile.open('SkyOS/initrd.tar', 'r') as tar:
    f = tar.extractfile('bin/init')
    data = f.read()
    print(f'Size: {len(data)} bytes')
    text = data.decode('latin-1')
    for m in re.finditer(r'[\x20-\x7e]{4,}', text):
        print(f'  String: {m.group()}')

    f2 = tar.extractfile('bin/echo')
    data2 = f2.read()
    print(f'\nEcho size: {len(data2)} bytes')
    text2 = data2.decode('latin-1')
    for m in re.finditer(r'[\x20-\x7e]{4,}', text2):
        print(f'  String: {m.group()}')
