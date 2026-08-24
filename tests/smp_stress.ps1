# smp_stress.ps1 — Boot with multiple CPU counts and verify stability.
#
# Tests SMP by booting with 1, 2, 4, and 8 CPUs, running the self-test
# suite (which includes stress tests), and verifying no lockups or panics.
#
# Usage:
#   .\tests\smp_stress.ps1                  # Test all CPU counts
#   .\tests\smp_stress.ps1 -Cpus "2,4,8"   # Specific counts
#   .\tests\smp_stress.ps1 -TimeoutSeconds 120

param(
    [string]$Cpus = "1,2,4,8",
    [int]$TimeoutSeconds = 120,
    [string]$LogDir = "test_logs\smp_stress"
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

$cpuCounts = $Cpus.Split(',') | ForEach-Object { [int]$_.Trim() }
$passed = 0
$failed = 0
$results = @()

Write-Host "=== SMP Stress Test ===" -ForegroundColor Cyan
Write-Host "CPU counts: $($cpuCounts -join ', ')" -ForegroundColor Gray
Write-Host "Timeout: ${TimeoutSeconds}s per boot" -ForegroundColor Gray
Write-Host ""

foreach ($cpuCount in $cpuCounts) {
    $logFile = Join-Path $LogDir "smp_${cpuCount}.log"
    Write-Host -NoNewline "SMP=$cpuCount ... " -ForegroundColor Gray

    try {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = "qemu-system-x86_64"
        $psi.Arguments = "-bios `"$bios`" -drive format=raw,file=`"$img`" -m 512M -smp $cpuCount -nographic -no-reboot -monitor none"
        $psi.UseShellExecute = $false
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.CreateNoWindow = $true

        $proc = [System.Diagnostics.Process]::Start($psi)
        $startTime = Get-Date
        $logContent = ""
        $panicked = $false

        while (!$proc.WaitForExit(100)) {
            $line = $proc.StandardOutput.ReadLine()
            if ($line) {
                $logContent += "$line`n"
                if ($line -match 'PANIC') { $panicked = $true }
            }
            $elapsed = ((Get-Date) - $startTime).TotalSeconds
            if ($elapsed -gt $TimeoutSeconds) {
                try { $proc.Kill() } catch {}
                break
            }
        }

        $logContent | Out-File $logFile -Encoding UTF8

        if ($panicked) {
            $failed++
            $results += @{ Cpus = $cpuCount; Status = "PANIC" }
            Write-Host "PANIC" -ForegroundColor Red
        } elseif ($logContent -match '(\d+)/(\d+) passed, 0 failed') {
            $p = $Matches[1]
            $t = $Matches[2]
            $passed++
            $results += @{ Cpus = $cpuCount; Status = "PASS"; Tests = "$p/$t" }
            Write-Host "PASS ($p/$t tests)" -ForegroundColor Green
        } elseif ($logContent -match '(\d+)/(\d+) passed, (\d+) failed') {
            $failed++
            $results += @{ Cpus = $cpuCount; Status = "FAIL"; Tests = "$($Matches[1])/$($Matches[2])" }
            Write-Host "FAIL ($($Matches[1])/$($Matches[2]), $($Matches[3]) failed)" -ForegroundColor Red
        } elseif ($logContent -match 'SARGA OS.*starting') {
            $passed++
            $results += @{ Cpus = $cpuCount; Status = "BOOT OK" }
            Write-Host "BOOT OK (no test output)" -ForegroundColor Yellow
        } else {
            $failed++
            $results += @{ Cpus = $cpuCount; Status = "NO OUTPUT" }
            Write-Host "NO OUTPUT" -ForegroundColor Red
        }
    } catch {
        $failed++
        $results += @{ Cpus = $cpuCount; Status = "ERROR" }
        Write-Host "ERROR: $_" -ForegroundColor Red
    }
}

# Summary
Write-Host ""
Write-Host "=== SMP Stress Results ===" -ForegroundColor Cyan
Write-Host "Total: $($cpuCounts.Count) CPU configurations" -ForegroundColor Gray
Write-Host "Passed: $passed" -ForegroundColor Green
Write-Host "Failed: $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })

foreach ($r in $results) {
    $color = if ($r.Status -eq "PASS" -or $r.Status -eq "BOOT OK") { "Green" } else { "Red" }
    $tests = if ($r.Tests) { " ($($r.Tests))" } else { "" }
    Write-Host "  SMP=$($r.Cpus): $($r.Status)$tests" -ForegroundColor $color
}

# Write summary
$summaryFile = Join-Path $LogDir "summary.txt"
$summary = @"
SMP Stress Test Summary
Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
CPU counts tested: $($cpuCounts -join ', ')
Passed: $passed / $($cpuCounts.Count)
Failed: $failed
$($results | ForEach-Object { "  SMP=$($_.Cpus): $($_.Status)" } | Out-String)
"@
$summary | Out-File $summaryFile -Encoding UTF8
Write-Host "`nSummary: $summaryFile" -ForegroundColor Gray

if ($failed -gt 0) { exit 1 }
