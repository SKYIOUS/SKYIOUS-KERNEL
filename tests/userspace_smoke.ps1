# userspace_smoke.ps1 — Boot QEMU, verify kernel boots and basic userspace markers.
#
# This script boots the kernel and checks serial output for:
# 1. Boot completion (BOOT markers)
# 2. Self-test pass (TAP output)
# 3. Userspace init spawn (if available)
# 4. GUI init (if framebuffer present)
#
# Usage:
#   .\tests\userspace_smoke.ps1
#   .\tests\userspace_smoke.ps1 -TimeoutSeconds 60
#   .\tests\userspace_smoke.ps1 -Smp 4

param(
    [int]$TimeoutSeconds = 90,
    [int]$Smp = 2,
    [string]$LogDir = "test_logs\userspace_smoke"
)

$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot | Split-Path -Parent
$bios = Join-Path $rootDir "OVMF.fd"
$img = Join-Path $rootDir "bootimage-vahi_kernel.bin"

if (!(Test-Path $img)) {
    Write-Host "ERROR: Boot image not found at $img" -ForegroundColor Red
    exit 1
}

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$logFile = Join-Path $LogDir "smoke_smp${Smp}.log"

Write-Host "=== Userspace Smoke Test (SMP=$Smp) ===" -ForegroundColor Cyan
Write-Host "Timeout: ${TimeoutSeconds}s" -ForegroundColor Gray
Write-Host ""

# Boot QEMU with serial piped to a file and also to stdout for live monitoring
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "qemu-system-x86_64"
$psi.Arguments = "-bios `"$bios`" -drive format=raw,file=`"$img`" -m 512M -smp $Smp -nographic -no-reboot -monitor none"
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($psi)

$checks = @{
    "boot_memory_init"  = $false
    "boot_apic_init"    = $false
    "boot_vfs_init"     = $false
    "boot_scheduler"    = $false
    "selftest_tap"      = $false
    "selftest_passed"   = $false
    "gui_init"          = $false
    "boot_complete"     = $false
}

$startTime = Get-Date
$allOutput = ""

try {
    while (!$proc.WaitForExit(100)) {
        # Read available output
        $line = $proc.StandardOutput.ReadLine()
        if ($line) {
            $allOutput += "$line`n"

            # Check for boot markers
            if ($line -match '\[BOOT\] memory::init done')     { $checks["boot_memory_init"] = $true }
            if ($line -match '\[BOOT\] APIC init')              { $checks["boot_apic_init"] = $true }
            if ($line -match '\[BOOT\] VFS init')               { $checks["boot_vfs_init"] = $true }
            if ($line -match '\[BOOT\] scheduler init')         { $checks["boot_scheduler"] = $true }
            if ($line -match 'TAP version 13')                  { $checks["selftest_tap"] = $true }
            if ($line -match '\d+/\d+ passed, 0 failed')        { $checks["selftest_passed"] = $true }
            if ($line -match '\[BOOT\] GUI init')               { $checks["gui_init"] = $true }
            if ($line -match 'SARGA OS.*starting')              { $checks["boot_complete"] = $true }

            # Print interesting lines
            if ($line -match '\[(BOOT|TEST|SELF-TEST|VERIFY|FAIL|PANIC)\]') {
                Write-Host "  $line" -ForegroundColor $(if ($line -match 'PANIC|FAIL') { "Red" } elseif ($line -match 'PASS') { "Green" } else { "Gray" })
            }
        }

        $elapsed = ((Get-Date) - $startTime).TotalSeconds
        if ($elapsed -gt $TimeoutSeconds) {
            Write-Host "TIMEOUT after ${TimeoutSeconds}s" -ForegroundColor Red
            break
        }
    }
} finally {
    try { $proc.Kill() } catch {}
}

# Save full log
$allOutput | Out-File $logFile -Encoding UTF8

# Report results
Write-Host ""
Write-Host "=== Check Results ===" -ForegroundColor Cyan

$passed = 0
$failed = 0
foreach ($check in $checks.GetEnumerator()) {
    $status = if ($check.Value) { "✅" } else { "❌" }
    $color = if ($check.Value) { "Green" } else { "Red" }
    Write-Host "  $status $($check.Key)" -ForegroundColor $color
    if ($check.Value) { $passed++ } else { $failed++ }
}

Write-Host ""
Write-Host "Passed: $passed/$($checks.Count)" -ForegroundColor $(if ($failed -eq 0) { "Green" } else { "Yellow" })

# Extract test results from log
if ($allOutput -match '(\d+)/(\d+) passed, (\d+) failed') {
    Write-Host "Self-test: $($Matches[1])/$($Matches[2]) passed, $($Matches[3]) failed" -ForegroundColor $(if ([int]$Matches[3] -eq 0) { "Green" } else { "Red" })
}

Write-Host "Full log: $logFile" -ForegroundColor Gray

if ($failed -gt 0) { exit 1 }
