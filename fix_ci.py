content = open('.github/workflows/vdso.yml').read()

old1 = """      - name: Run QEMU self-tests
        id: qemu_test
        run: |
          python3 tests/run_qemu_tests.py \\
            --image bootimage-vahi_kernel.bin \\
            --timeout 540 \\
            --log qemu_test.log
        continue-on-error: true"""

new1 = """      - name: Run QEMU self-tests
        id: qemu_test
        run: |
          python3 tests/run_qemu_tests.py \\
            --image bootimage-vahi_kernel.bin \\
            --timeout 540 \\
            --log qemu_test.log
          if ! grep -q "TAP version" qemu_test.log; then
            echo "ERROR: No TAP output found"
            exit 1
          fi
          if grep -q "not ok.*vdso" qemu_test.log; then
            echo "ERROR: VDSO tests failed"
            exit 1
          fi
          VDSO_PASS=$(grep -c "ok.*vdso" qemu_test.log || true)
          if [ "$VDSO_PASS" -lt 25 ]; then
            echo "ERROR: Only $VDSO_PASS vDSO tests passed (expected 25+)"
            exit 1
          fi
          echo "VDSO tests: $VDSO_PASS passed" """

assert old1 in content, 'old1 not found'
content = content.replace(old1, new1)

content = content.replace(
    'UNSAFE_WITHOUT_SAFETY=$(grep -r "unsafe {" --include="*.rs" kernel/src | grep -v "// SAFETY:" | grep -v "//SAFETY:" | wc -l)',
    'UNSAFE_WITHOUT_SAFETY=$(grep -r "unsafe {" --include="*.rs" kernel/src | grep -v "// SAFETY:" | grep -vc "//SAFETY:" || true)')
content = content.replace(
    'PRINTLN_COUNT=$(grep -r "println!" --include="*.rs" kernel/src | grep -v "test" | grep -v "// " | wc -l)',
    'PRINTLN_COUNT=$(grep -r "println!" --include="*.rs" kernel/src | grep -v "test" | grep -vc "// " || true)')
content = content.replace(
    'STD_IMPORTS=$(grep -r "use std::" --include="*.rs" kernel/src | grep -v "test" | wc -l)',
    'STD_IMPORTS=$(grep -r "use std::" --include="*.rs" kernel/src | grep -v "test" | grep -vc "test" || true)')

open('.github/workflows/vdso.yml', 'w').write(content)
print('done')
