# boot_reliability.ps1 — Run N consecutive QEMU boots and count passes/failures.
#
# Usage:
#   .\tests\boot_reliability.ps1              # 100 boots, default log dir
#   .\tests\boot_reliability.ps1 -Count 50    # 50 boots
#   .\tests\boot_reliability.ps1 -Count 10 -TimeoutSeconds 60
#
# Requires: QEMU, OVMF.fd, bootimage-vahi_kernel.bin
# The bootimage must be built with --features self_test for test output.

param(
    [int]$Count = 100,
    [int]$TimeoutSeconds = 120,
    [string]$LogDir = "test_logs\boot_reliability"
)

$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot | Split-Path -Parent
$bios = Join-Path $rootDir "OVMF.fd"
$img = Join-Path $rootDir "bootimage-vahi_kernel.bin"

if (!(Test-Path $img)) {
    Write-Host "ERROR: Boot image not found at $img" -ForegroundColor Red
    Write-Host "Run .\make_bootimage.ps1 first (with --features self_test)" -ForegroundColor Yellow
    exit 1
}
if (!(Test-Path $bios)) {
    Write-Host "ERROR: OVMF.fd not found at $bios" -ForegroundColor Red
    exit 1
}

# Create log directory
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

$passed = 0
$failed = 0
$failures = @()

Write-Host "=== Boot Reliability Test ===" -ForegroundColor Cyan
Write-Host "Count: $Count boots, Timeout: ${TimeoutSeconds}s each" -ForegroundColor Gray
Write-Host "Log directory: $LogDir" -ForegroundColor Gray
Write-Host ""

for ($i = 1; $i -le $Count; $i++) {
    $logFile = Join-Path $LogDir "boot_${i}.log"
    $padded = $i.ToString().PadLeft(3, '0')

    Write-Host -NoNewline "Boot $padded/$Count ... " -ForegroundColor Gray

    try {
        # Start QEMU in background, capture serial to file
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = "qemu-system-x86_64"
        $psi.Arguments = "-bios `"$bios`" -drive format=raw,file=`"$img`" -m 512M -smp 2 -nographic -serial file:`"$logFile`" -no-reboot -monitor none"
        $psi.UseShellExecute = $false
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.CreateNoWindow = $true

        $proc = [System.Diagnostics.Process]::Start($psi)

        # Wait for QEMU to finish (timeout)
        $exited = $proc.WaitForExit($TimeoutSeconds * 1000)

        if (!$exited) {
            # Timeout — kill the process
            try { $proc.Kill() } catch {}
            $failed++
            $failures += "$padded (TIMEOUT)"
            Write-Host "TIMEOUT" -ForegroundColor Red
            continue
        }

        # Check serial log for test results
        $logContent = ""
        if (Test-Path $logFile) {
            $logContent = Get-Content $logFile -Raw -ErrorAction SilentlyContinue
        }

        # Look for TAP output: "N passed, 0 failed" or "N/N passed, 0 failed"
        if ($logContent -match '(\d+)/(\d+) passed, (\d+) failed') {
            $p = [int]$Matches[1]
            $t = [int]$Matches[2]
            $f = [int]$Matches[3]
            if ($f -eq 0) {
                $passed++
                Write-Host "PASS ($p/$t)" -ForegroundColor Green
            } else {
                $failed++
                $failures += "$padded ($f failures)"
                Write-Host "FAIL ($p/$t, $f failed)" -ForegroundColor Red
            }
        } elseif ($logContent -match '(\d+) passed, (\d+) failed') {
            $p = [int]$Matches[1]
            $f = [int]$Matches[2]
            if ($f -eq 0) {
                $passed++
                Write-Host "PASS ($p passed)" -ForegroundColor Green
            } else {
                $failed++
                $failures += "$padded ($f failures)"
                Write-Host "FAIL ($p passed, $f failed)" -ForegroundColor Red
            }
        } elseif ($logContent -match 'PANIC') {
            $failed++
            $failures += "$padded (PANIC)"
            Write-Host "PANIC" -ForegroundColor Red
        } elseif ($logContent -match 'BOOT') {
            # Booted but no test output — partial success
            $passed++
            Write-Host "BOOT OK (no test output)" -ForegroundColor Yellow
        } else {
            $failed++
            $failures += "$padded (no recognizable output)"
            Write-Host "NO OUTPUT" -ForegroundColor Red
        }
    } catch {
        $failed++
        $failures += "$padded (exception: $_)"
        Write-Host "ERROR: $_" -ForegroundColor Red
    }
}

# Summary
Write-Host ""
Write-Host "=== Results ===" -ForegroundColor Cyan
Write-Host "Total: $Count" -ForegroundColor Gray
Write-Host "Passed: $passed" -ForegroundColor Green
Write-Host "Failed: $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })
Write-Host "Pass rate: $([math]::Round($passed / $Count * 100, 1))%" -ForegroundColor $(if ($failed -eq 0) { "Green" } else { "Yellow" })

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Failed boots:" -ForegroundColor Red
    foreach ($f in $failures) {
        Write-Host "  - Boot $f" -ForegroundColor Red
    }
}

# Write summary to file
$summaryFile = Join-Path $LogDir "summary.txt"
$summary = @"
Boot Reliability Test Summary
Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Total: $Count
Passed: $passed
Failed: $failed
Pass rate: $([math]::Round($passed / $Count * 100, 1))%
Timeout: ${TimeoutSeconds}s per boot
$(
    if ($failures.Count -gt 0) {
        "`nFailed boots:`n" + ($failures | ForEach-Object { "  - $_" } | Out-String)
    }
)
"@
$summary | Out-File $summaryFile -Encoding UTF8
Write-Host "`nSummary written to: $summaryFile" -ForegroundColor Gray

# Exit with failure code if any boots failed
if ($failed -gt 0) { exit 1 }
