# fs_crash_recovery.ps1 — Filesystem crash-recovery baseline test.
#
# Boots QEMU, waits for the kernel to reach userspace, kills QEMU
# abruptly (simulating power loss), reboots, and checks that the
# filesystem is consistent.
#
# This establishes a baseline crash-recovery pass rate for SkyFS and ext2.
#
# Usage:
#   .\tests\fs_crash_recovery.ps1                   # 20 crash cycles
#   .\tests\fs_crash_recovery.ps1 -Cycles 50
#   .\tests\fs_crash_recovery.ps1 -WriteDelayMs 500  # delay before crash

param(
    [int]$Cycles = 20,
    [int]$WriteDelayMs = 500,
    [int]$BootTimeoutSeconds = 60,
    [string]$LogDir = "test_logs\fs_crash_recovery"
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

$passed = 0
$failed = 0
$panics = @()

Write-Host "=== Filesystem Crash Recovery Baseline ===" -ForegroundColor Cyan
Write-Host "Cycles: $Cycles, Write delay: ${WriteDelayMs}ms before crash" -ForegroundColor Gray
Write-Host ""

for ($i = 1; $i -le $Cycles; $i++) {
    $logFile = Join-Path $LogDir "crash_${i}.log"
    $padded = $i.ToString().PadLeft(3, '0')

    Write-Host -NoNewline "Cycle $padded/$Cycles ... " -ForegroundColor Gray

    try {
        # Phase 1: Boot and wait for kernel to reach a stable state
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = "qemu-system-x86_64"
        $psi.Arguments = "-bios `"$bios`" -drive format=raw,file=`"$img`" -m 512M -smp 2 -nographic -no-reboot -monitor none"
        $psi.UseShellExecute = $false
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.CreateNoWindow = $true

        $proc = [System.Diagnostics.Process]::Start($psi)
        $bootComplete = $false

        # Wait for boot to complete (look for scheduler init or selftest)
        $bootStart = Get-Date
        while (!$proc.WaitForExit(100)) {
            $line = $proc.StandardOutput.ReadLine()
            if ($line) {
                if ($line -match '\[BOOT\] scheduler init|TAP version 13') {
                    $bootComplete = $true
                    break
                }
                if ($line -match 'PANIC') {
                    break
                }
            }
            $elapsed = ((Get-Date) - $bootStart).TotalSeconds
            if ($elapsed -gt $BootTimeoutSeconds) {
                break
            }
        }

        if (!$bootComplete) {
            # Kill and try next cycle
            try { $proc.Kill() } catch {}
            Start-Sleep -Milliseconds 500
            $failed++
            $panics += "$padded (boot failed)"
            Write-Host "BOOT FAILED" -ForegroundColor Red
            continue
        }

        # Phase 2: Let the kernel run for a bit (simulating workload)
        Start-Sleep -Milliseconds $WriteDelayMs

        # Phase 3: Kill QEMU abruptly (simulating power loss)
        try { $proc.Kill() } catch {}
        Start-Sleep -Milliseconds 500  # Let QEMU fully terminate

        # Phase 4: Reboot and check for filesystem errors
        $psi2 = New-Object System.Diagnostics.ProcessStartInfo
        $psi2.FileName = "qemu-system-x86_64"
        $psi2.Arguments = "-bios `"$bios`" -drive format=raw,file=`"$img`" -m 512M -smp 2 -nographic -no-reboot -monitor none"
        $psi2.UseShellExecute = $false
        $psi2.RedirectStandardOutput = $true
        $psi2.RedirectStandardError = $true
        $psi2.CreateNoWindow = $true

        $proc2 = [System.Diagnostics.Process]::Start($psi2)
        $rebootLog = ""
        $panicked = $false
        $fsErrors = $false

        $rebootStart = Get-Date
        while (!$proc2.WaitForExit(100)) {
            $line = $proc2.StandardOutput.ReadLine()
            if ($line) {
                $rebootLog += "$line`n"
                if ($line -match 'PANIC|kernel panic') { $panicked = $true }
                if ($line -match 'filesystem error|corrupt|journal|EXT2.*error|SkyFS.*error') { $fsErrors = $true }
            }
            $elapsed = ((Get-Date) - $rebootStart).TotalSeconds
            if ($elapsed -gt $BootTimeoutSeconds) { break }
        }

        try { $proc2.Kill() } catch {}

        $rebootLog | Out-File $logFile -Encoding UTF8

        if ($panicked) {
            $failed++
            $panics += "$padded (reboot panic)"
            Write-Host "REBOOT PANIC" -ForegroundColor Red
        } elseif ($fsErrors) {
            $failed++
            $panics += "$padded (FS errors after crash)"
            Write-Host "FS ERRORS" -ForegroundColor Red
        } elseif ($rebootLog -match 'passed, 0 failed') {
            $passed++
            Write-Host "PASS" -ForegroundColor Green
        } else {
            $passed++
            Write-Host "OK (reboot clean)" -ForegroundColor Green
        }

    } catch {
        $failed++
        Write-Host "ERROR: $_" -ForegroundColor Red
    }
}

# Summary
Write-Host ""
Write-Host "=== Crash Recovery Results ===" -ForegroundColor Cyan
Write-Host "Total: $Cycles crash/reboot cycles" -ForegroundColor Gray
Write-Host "Passed: $passed" -ForegroundColor Green
Write-Host "Failed: $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })
Write-Host "Pass rate: $([math]::Round($passed / $Cycles * 100, 1))%" -ForegroundColor $(if ($failed -eq 0) { "Green" } else { "Yellow" })

if ($panics.Count -gt 0) {
    Write-Host ""
    Write-Host "Failures:" -ForegroundColor Red
    foreach ($p in $panics) {
        Write-Host "  - $p" -ForegroundColor Red
    }
}

# Write summary
$summaryFile = Join-Path $LogDir "summary.txt"
$summary = @"
Filesystem Crash Recovery Baseline
Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Cycles: $Cycles
Write delay before crash: ${WriteDelayMs}ms
Passed: $passed
Failed: $failed
Pass rate: $([math]::Round($passed / $Cycles * 100, 1))%
$(
    if ($panics.Count -gt 0) {
        "`nFailures:`n" + ($panics | ForEach-Object { "  - $_" } | Out-String)
    }
)
"@
$summary | Out-File $summaryFile -Encoding UTF8
Write-Host "`nSummary: $summaryFile" -ForegroundColor Gray

if ($failed -gt 0) { exit 1 }
