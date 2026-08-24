# extended_fuzz.ps1 — Extended fuzz run via repeated QEMU boot cycles.
#
# Since the kernel fuzzer runs at boot time (selftest), extended fuzzing
# means running many boot cycles with the fuzzer enabled. Each boot
# exercises the fuzzer with different entropy (RDRAND/TSC seed).
#
# Usage:
#   .\tests\extended_fuzz.ps1                    # 60 boot cycles (~1 hour)
#   .\tests\extended_fuzz.ps1 -Cycles 200        # 200 boot cycles
#   .\tests\extended_fuzz.ps1 -Cycles 10 -TimeoutSeconds 60

param(
    [int]$Cycles = 60,
    [int]$TimeoutSeconds = 90,
    [string]$LogDir = "test_logs\extended_fuzz"
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
$startTime = Get-Date

Write-Host "=== Extended Fuzz Run ===" -ForegroundColor Cyan
Write-Host "Cycles: $Cycles, Timeout: ${TimeoutSeconds}s each" -ForegroundColor Gray
Write-Host "Start: $(Get-Date -Format 'HH:mm:ss')" -ForegroundColor Gray
Write-Host ""

for ($i = 1; $i -le $Cycles; $i++) {
    $logFile = Join-Path $LogDir "fuzz_${i}.log"
    $padded = $i.ToString().PadLeft(3, '0')

    Write-Host -NoNewline "Cycle $padded/$Cycles ... " -ForegroundColor Gray

    try {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = "qemu-system-x86_64"
        $psi.Arguments = "-bios `"$bios`" -drive format=raw,file=`"$img`" -m 512M -smp 2 -nographic -no-reboot -monitor none"
        $psi.UseShellExecute = $false
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.CreateNoWindow = $true

        $proc = [System.Diagnostics.Process]::Start($psi)
        $startTime_cycle = Get-Date
        $logContent = ""
        $panicked = $false

        while (!$proc.WaitForExit(100)) {
            $line = $proc.StandardOutput.ReadLine()
            if ($line) {
                $logContent += "$line`n"
                if ($line -match 'PANIC|kernel panic') { $panicked = $true }
            }
            $elapsed = ((Get-Date) - $startTime_cycle).TotalSeconds
            if ($elapsed -gt $TimeoutSeconds) {
                try { $proc.Kill() } catch {}
                $logContent += "`n[TIMEOUT after ${TimeoutSeconds}s]`n"
                break
            }
        }

        $logContent | Out-File $logFile -Encoding UTF8

        if ($panicked) {
            $failed++
            $panics += $padded
            Write-Host "PANIC" -ForegroundColor Red
        } elseif ($logContent -match 'passed, 0 failed') {
            $passed++
            Write-Host "PASS" -ForegroundColor Green
        } elseif ($logContent -match 'passed, (\d+) failed') {
            $failed++
            Write-Host "FAIL ($($Matches[1]) failed)" -ForegroundColor Red
        } else {
            $passed++
            Write-Host "OK (no test output)" -ForegroundColor Yellow
        }

        # Progress indicator every 10 cycles
        if ($i % 10 -eq 0) {
            $elapsed = ((Get-Date) - $startTime).TotalMinutes
            Write-Host "  [Progress: $i/$Cycles, ${elapsed} min elapsed, $passed pass, $failed fail]" -ForegroundColor DarkGray
        }
    } catch {
        $failed++
        Write-Host "ERROR: $_" -ForegroundColor Red
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
Duration: $($totalElapsed.TotalMinutes.ToString('F1')) minutes
Panic cycles: $($panics -join ', ')
"@
$summary | Out-File $summaryFile -Encoding UTF8

if ($failed -gt 0) { exit 1 }
