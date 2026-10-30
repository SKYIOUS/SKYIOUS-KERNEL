# extended_fuzz.ps1 - Extended fuzz run via repeated QEMU boot cycles using Python runner.
#
# Since the kernel fuzzer runs at boot time (selftest), extended fuzzing
# means running many boot cycles with the fuzzer enabled. Each boot
# exercises the fuzzer with different entropy (RDRAND/TSC seed).
#
# Usage:
#   .\tests\extended_fuzz.ps1                    # 60 boot cycles (~1 hour)
#   .\tests\extended_fuzz.ps1 -Cycles 200        # 200 boot cycles
#   .\tests\extended_fuzz.ps1 -Cycles 10 -TimeoutSeconds 60
#
# Uses the Python test runner which correctly parses TAP and kills QEMU
# after the verdict, avoiding the idle-loop hang of raw QEMU execution.
param(
    [int]$Cycles = 60,
    [int]$TimeoutSeconds = 300,
    [string]$LogDir = "test_logs\extended_fuzz",
    [string]$Image = "tests/t00_selftest.bin"
)

$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot | Split-Path -Parent
$img = Join-Path $rootDir $Image

if (!(Test-Path $img)) {
    Write-Host "ERROR: Boot image not found at $img" -ForegroundColor Red
    exit 1
}

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

$passed = 0
$failed = 0
$panics = @()
$startTime = Get-Date

Write-Host "=== Extended Fuzz Run ===" -ForegroundColor Cyan
Write-Host "Cycles: $Cycles, Timeout: ${TimeoutSeconds}s each" -ForegroundColor Gray
Write-Host "Image: $Image" -ForegroundColor Gray
Write-Host "Start: $(Get-Date -Format 'HH:mm:ss')" -ForegroundColor Gray
Write-Host ""

for ($i = 1; $i -le $Cycles; $i++) {
    $padded = $i.ToString().PadLeft(3, '0')
    $logFile = Join-Path $LogDir "fuzz_${padded}.log"

    Write-Host -NoNewline "Cycle $padded/$Cycles ... " -ForegroundColor Gray

    $args = @(
        "--image", $img,
        "--timeout", $TimeoutSeconds.ToString(),
        "--smp", "2",
        "--qemu-extra", "-vga none"
    )

    try {
        $result = py -3 tests/run_qemu_tests.py @args 2>&1
        $result | Out-File $logFile -Encoding UTF8

        if ($result -match 'RESULT: PASS') {
            Write-Host "PASS" -ForegroundColor Green
            $passed++
        }
        elseif ($result -match 'RESULT: PANIC') {
            Write-Host "PANIC" -ForegroundColor Red
            $failed++
            $panics += $padded
        }
        elseif ($result -match 'RESULT: FAIL') {
            Write-Host "FAIL" -ForegroundColor Red
            $failed++
        }
        elseif ($result -match 'RESULT: (BOOT_FAILURE|UNEXPECTED_EXIT|TIMEOUT|INFRASTRUCTURE_ERROR)') {
            $reason = $matches[1]
            Write-Host "$reason" -ForegroundColor Red
            $failed++
        }
        else {
            Write-Host "UNKNOWN RESULT" -ForegroundColor Red
            Write-Host $result
            $failed++
        }
    }
    catch {
        Write-Host "ERROR: $_" -ForegroundColor Red
        $failed++
    }

    if ($i % 10 -eq 0) {
        $elapsed = ((Get-Date) - $startTime).TotalMinutes
        Write-Host "  [Progress: $i/$Cycles, ${elapsed} min elapsed, $passed pass, $failed fail]" -ForegroundColor DarkGray
    }
}

# Summary
$totalElapsed = ((Get-Date) - $startTime)
Write-Host ""
Write-Host "=== Extended Fuzz Results ===" -ForegroundColor Cyan
Write-Host "Cycles: $Cycles" -ForegroundColor Gray
Write-Host "Passed: $passed" -ForegroundColor Green
Write-Host "Failed: $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })
Write-Host "Duration: $([math]::Round($totalElapsed.TotalMinutes, 1)) minutes" -ForegroundColor Gray

if ($panics.Count -gt 0) {
    Write-Host ""
    Write-Host "Panics in cycles: $($panics -join ', ')" -ForegroundColor Red
    Write-Host "Check logs in $LogDir for details" -ForegroundColor Yellow
}

# Write summary
$summaryFile = Join-Path $LogDir "summary.txt"
$summary = @"
Extended Fuzz Run Summary
Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Cycles: $Cycles
Passed: $passed
Failed: $failed
Duration: $([math]::Round($totalElapsed.TotalMinutes, 1)) minutes
Panic cycles: $($panics -join ', ')
"@
$summary | Out-File $summaryFile -Encoding UTF8

if ($failed -gt 0) { exit 1 }
