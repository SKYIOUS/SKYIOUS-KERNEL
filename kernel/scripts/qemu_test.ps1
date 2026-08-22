#!/usr/bin/env pwsh
<#
.SYNOPSIS
    QEMU integration test runner for the Vahi kernel.

.DESCRIPTION
    Builds the kernel with self_test feature, creates a bootable image,
    launches QEMU, captures serial output, and parses TAP results.

.PARAMETER Release
    Use release profile (optimized, slower build).

.PARAMETER Smp
    Number of QEMU CPUs (default: 1).

.PARAMETER Timeout
    QEMU timeout in seconds (default: 120).

.PARAMETER KeepImage
    Don't delete boot image after test.

.PARAMETER QemuExtra
    Extra QEMU arguments (e.g., "-accel tcg,thread=4").

.EXAMPLE
    ./scripts/qemu_test.ps1
    ./scripts/qemu_test.ps1 -Release -Smp 4
    ./scripts/qemu_test.ps1 -Timeout 300 -QemuExtra "-accel kvm"
#>

param(
    [switch]$Release,
    [int]$Smp = 1,
    [int]$Timeout = 120,
    [switch]$KeepImage,
    [string]$QemuExtra = ""
)

$ErrorActionPreference = "Stop"
$KernelDir = Join-Path $PSScriptRoot ".."
$TestDir = Join-Path $PSScriptRoot "..\target\qemu_tests"

# ── Helpers ───────────────────────────────────────────────────────────

function Write-Step($msg) { Write-Host ">>> $msg" -ForegroundColor Cyan }
function Write-OK($msg)   { Write-Host "✅ $msg" -ForegroundColor Green }
function Write-Fail($msg) { Write-Host "❌ $msg" -ForegroundColor Red }
function Write-Warn($msg) { Write-Host "⚠️  $msg" -ForegroundColor Yellow }

function Parse-Tap($output) {
    $result = @{
        Version = $null
        Planned = $null
        Passed  = @()
        Failed  = @()
        BailOut = $false
    }

    foreach ($line in $output -split "`n") {
        $line = $line.Trim()

        if ($line -match '^TAP version (\d+)') {
            $result.Version = [int]$Matches[1]
        }
        elseif ($line -match '^1\.\.(\d+)') {
            $result.Planned = [int]$Matches[1]
        }
        elseif ($line -match '^Bail out') {
            $result.BailOut = $true
        }
        elseif ($line -match '^ok (\d+) - (.+)') {
            $result.Passed += $Matches[2].Trim()
        }
        elseif ($line -match '^not ok (\d+) - (.+)') {
            $result.Failed += $Matches[2].Trim()
        }
    }

    return $result
}

# ── Main ──────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "╔══════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║   Vahi Kernel — QEMU Integration Tests      ║" -ForegroundColor Cyan
Write-Host "╚══════════════════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

# Check prerequisites
$QemuPath = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
if (-not $QemuPath) {
    Write-Fail "qemu-system-x86_64 not found in PATH"
    Write-Host "Install QEMU: https://www.qemu.org/download/"
    exit 1
}
Write-OK "QEMU found: $($QemuPath.Source)"

# Step 1: Build kernel
Write-Step "Building kernel with self_test feature..."
$ProfileFlag = if ($Release) { "--release" } else { "" }
$BuildCmd = "cargo build $ProfileFlag --features self_test --target x86_64-unknown-none"
$BuildResult = Start-Process -FilePath "cargo" -ArgumentList "build", $ProfileFlag, "--features", "self_test", "--target", "x86_64-unknown-none" -WorkingDirectory $KernelDir -Wait -PassThru -NoNewWindow

if ($BuildResult.ExitCode -ne 0) {
    Write-Fail "Kernel build failed (exit code $($BuildResult.ExitCode))"
    exit 1
}
Write-OK "Kernel built successfully"

# Step 2: Create bootable image
Write-Step "Creating bootable image..."
$BootImageResult = Start-Process -FilePath "cargo" -ArgumentList "bootimage" -WorkingDirectory $KernelDir -Wait -PassThru -NoNewWindow

if ($BootImageResult.ExitCode -ne 0) {
    Write-Fail "bootimage failed (exit code $($BootImageResult.ExitCode))"
    Write-Host "Install bootimage: cargo install bootimage"
    exit 1
}

# Find the boot image
$TargetDir = if ($Release) { "target/x86_64-unknown-none/release" } else { "target/x86_64-unknown-none/debug" }
$BootImage = Get-ChildItem -Path (Join-Path $KernelDir $TargetDir) -Filter "bootimage-*.bin" | Select-Object -First 1

if (-not $BootImage) {
    $BootImage = Get-ChildItem -Path (Join-Path $KernelDir $TargetDir) -Filter "*boot_image*.img" | Select-Object -First 1
}

if (-not $BootImage) {
    Write-Fail "No boot image found in $TargetDir"
    exit 1
}

Write-OK "Boot image: $($BootImage.Name) ($([math]::Round($BootImage.Length / 1MB, 1)) MB)"

# Step 3: Run QEMU
Write-Step "Launching QEMU (timeout: ${Timeout}s, SMP: $Smp)..."

$QemuArgs = @(
    "-drive", "format=raw,file=$($BootImage.FullName)",
    "-m", "512",
    "-nographic",
    "-serial", "stdio",
    "-no-reboot",
    "-d", "guest_errors",
    "-smp", $Smp
)

if ($QemuExtra) {
    $QemuArgs += $QemuExtra -split " "
}

$StartTime = Get-Date
$SerialOutput = ""

try {
    $QemuProcess = Start-Process -FilePath "qemu-system-x86_64" -ArgumentList $QemuArgs -Wait -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $TestDir "serial.log") -RedirectStandardError (Join-Path $TestDir "qemu_stderr.log")
    $ExitCode = $QemuProcess.ExitCode
} catch {
    Write-Fail "Failed to start QEMU: $_"
    exit 1
}

$Elapsed = (Get-Date) - $StartTime
$SerialOutput = Get-Content (Join-Path $TestDir "serial.log") -Raw -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Serial output:" -ForegroundColor Gray
Write-Host $SerialOutput -ForegroundColor DarkGray

# Step 4: Parse TAP results
Write-Step "Parsing TAP results..."
$Tap = Parse-Tap $SerialOutput

# Display results
if ($Tap.Passed.Count -gt 0) {
    Write-Host ""
    Write-Host "PASSED tests:" -ForegroundColor Green
    foreach ($t in $Tap.Passed) {
        Write-Host "  ✓ $t" -ForegroundColor Green
    }
}

if ($Tap.Failed.Count -gt 0) {
    Write-Host ""
    Write-Host "FAILED tests:" -ForegroundColor Red
    foreach ($t in $Tap.Failed) {
        Write-Host "  ✗ $t" -ForegroundColor Red
    }
}

# Step 5: Summary
Write-Host ""
Write-Host "═══════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  TAP Version:  $($Tap.Version ?? 'N/A')"
Write-Host "  Tests passed: $($Tap.Passed.Count)"
Write-Host "  Tests failed: $($Tap.Failed.Count)"
Write-Host "  Plan:         1..$($Tap.Planned ?? 'N/A')"
Write-Host "  Elapsed:      $([math]::Round($Elapsed.TotalSeconds, 1))s"
Write-Host "  QEMU exit:    $ExitCode"
Write-Host "═══════════════════════════════════════════════" -ForegroundColor Cyan

if ($Tap.BailOut) {
    Write-Fail "Kernel bailed out during selftest!"
    exit 1
}

if ($Tap.Failed.Count -gt 0) {
    Write-Fail "$($Tap.Failed.Count) selftest(s) FAILED"
    exit 1
}

if ($Tap.Passed.Count -eq 0) {
    Write-Warn "No TAP tests found in output — kernel may not have booted correctly"
    exit 1
}

Write-OK "All $($Tap.Passed.Count) selftests passed!"
exit 0
