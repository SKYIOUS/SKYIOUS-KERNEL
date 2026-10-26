# verify.ps1 — Local mirror of the CI gates.
#
# Usage:
#   .\scripts\verify.ps1              # format, policy, build, clippy, QEMU
#   .\scripts\verify.ps1 -SkipQemu    # host-only gates
param([switch]$SkipQemu)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$KernelDir = Join-Path $RootDir "kernel"
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path $CargoBin) { $env:Path = "$CargoBin;$env:Path" }
$Python = if (Get-Command python -ErrorAction SilentlyContinue) { "python" } else { "py" }
$Features = "self_test,smp,net,ext4,iommu"
$Target = "x86_64-unknown-none"
$Artifacts = Join-Path $env:TEMP "vahi-verify"
New-Item -ItemType Directory -Force -Path $Artifacts | Out-Null

function Invoke-Gate {
    param([string]$Name, [string]$Dir, [scriptblock]$Action)
    Write-Host "=== $Name ===" -ForegroundColor Cyan
    Push-Location $Dir
    try { & $Action } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "$Name failed (exit $LASTEXITCODE)" }
}

Invoke-Gate "fmt" $RootDir { cargo fmt --all -- --check }
Invoke-Gate "policy self-test" $RootDir { & $Python scripts/check_repo_policy.py --self-test }
Invoke-Gate "policy (worktree)" $RootDir { & $Python scripts/check_repo_policy.py --worktree }
Invoke-Gate "build (release, self_test)" $KernelDir {
    cargo build --release --target $Target --features $Features --locked
}
Invoke-Gate "clippy (release, self_test)" $KernelDir {
    cargo clippy --release --target $Target --features $Features --locked -- -D warnings
}

if ($SkipQemu) {
    Write-Host "=== QEMU gates skipped (-SkipQemu) ===" -ForegroundColor Yellow
    exit 0
}

$QemuTests = Join-Path $RootDir "tests/run_qemu_tests.py"
$ReleaseKernel = Join-Path $RootDir "target/$Target/release/vahi_kernel"
$ReleaseImage = Join-Path $Artifacts "selftest.bin"

Invoke-Gate "image (release, self_test)" $RootDir {
    & $Python builder/build_limine_image.py --kernel $ReleaseKernel --initrd kernel/initrd.tar --output $ReleaseImage
}
Invoke-Gate "QEMU SMP 1" $RootDir {
    & $Python $QemuTests --image $ReleaseImage --expect-count auto --timeout 900 `
        --qemu-extra "-vga none" `
        --serial (Join-Path $Artifacts "smp1_serial.log") --log (Join-Path $Artifacts "smp1.log")
}
Invoke-Gate "QEMU SMP 2" $RootDir {
    & $Python $QemuTests --image $ReleaseImage --expect-count auto --smp 2 --timeout 900 `
        --qemu-extra "-cpu qemu64,-smep -vga none" `
        --serial (Join-Path $Artifacts "smp2_serial.log") --log (Join-Path $Artifacts "smp2.log")
}

Invoke-Gate "build (audit_small, self_test)" $KernelDir {
    cargo build --profile audit_small --target $Target --features $Features --locked
}
Invoke-Gate "clippy (audit_small, self_test)" $KernelDir {
    cargo clippy --profile audit_small --target $Target --features $Features --locked -- -D warnings
}
$AuditKernel = Join-Path $RootDir "target/$Target/audit_small/vahi_kernel"
$AuditImage = Join-Path $Artifacts "audit_small.bin"
Invoke-Gate "image (audit_small, self_test)" $RootDir {
    & $Python builder/build_limine_image.py --kernel $AuditKernel --initrd kernel/initrd.tar --output $AuditImage
}
Invoke-Gate "QEMU audit_small" $RootDir {
    & $Python $QemuTests --image $AuditImage --expect-count auto --timeout 900 `
        --qemu-extra "-vga none" `
        --serial (Join-Path $Artifacts "audit_serial.log") --log (Join-Path $Artifacts "audit.log")
}

Write-Host "=== All gates passed (logs: $Artifacts) ===" -ForegroundColor Green
