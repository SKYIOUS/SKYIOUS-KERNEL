# dev.ps1 — Daily development loop: fmt → policy → build → clippy → host tests
# Usage: .\scripts\dev.ps1 [-Quick]  (-Quick skips host tests)

param([switch]$Quick)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$KernelDir = Join-Path $RootDir "kernel"
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path $CargoBin) { $env:Path = "$CargoBin;$env:Path" }
$Python = if (Get-Command python -ErrorAction SilentlyContinue) { "python" } else { "py" }

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
    cargo build --release --target x86_64-unknown-none --features "self_test,smp,net,ext4,iommu" --locked
}
Invoke-Gate "clippy (release, self_test)" $KernelDir {
    cargo clippy --release --target x86_64-unknown-none --features "self_test,smp,net,ext4,iommu" --locked -- -D warnings
}

if (-not $Quick) {
    Invoke-Gate "host tests (default features)" $RootDir { cargo test --workspace --exclude vahi_kernel --locked }
    Invoke-Gate "host tests (all features)" $RootDir { cargo test --workspace --exclude vahi_kernel --all-features --locked }
    Invoke-Gate "host clippy (all features)" $RootDir { cargo clippy --workspace --exclude vahi_kernel --all-targets --all-features --locked -- -D warnings }
}

Write-Host "=== All dev gates passed ===" -ForegroundColor Green