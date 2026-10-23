# boot_stress_100.ps1 — Run 100 consecutive QEMU boots via the Python runner.
# Usage: .\tests\boot_stress_100.ps1 [-Timeout 300] [-Smp 1] [-Tries 100]
#        [-Image tests/t00_selftest.bin]
#
# Uses the Python test runner which correctly parses TAP and kills QEMU
# after the verdict, avoiding the idle-loop hang of raw QEMU execution.
param(
    [int]$Timeout = 300,
    [int]$Smp = 1,
    [int]$Tries = 100,
    [string]$Image = "tests/t00_selftest.bin"
)

if (-not (Test-Path $Image)) {
    Write-Host "ERROR: $Image not found. Build the selftest image first." -ForegroundColor Red
    exit 1
}

Write-Host "Boot stress test: $Tries tries, SMP=$Smp, timeout=${Timeout}s" -ForegroundColor Cyan

$Pass = 0
$Fail = 0
$PanicCycles = @()

for ($i = 1; $i -le $Tries; $i++) {
    Write-Host -NoNewline "  [$i/$Tries] " -ForegroundColor Gray

    $args = @(
        "--image", $Image,
        "--timeout", $Timeout.ToString(),
        "--smp", $Smp,
        "--qemu-extra", "-vga none"
    )

    try {
        $result = py -3 tests/run_qemu_tests.py @args 2>&1
        $exitCode = $LASTEXITCODE

        if ($result -match 'RESULT: PASS') {
            Write-Host "PASS" -ForegroundColor Green
            $Pass++
        }
        elseif ($result -match 'RESULT: PANIC') {
            Write-Host "PANIC" -ForegroundColor Red
            $Fail++
            $PanicCycles += $i.ToString().PadLeft(3, '0')
        }
        elseif ($result -match 'RESULT: FAIL') {
            Write-Host "FAIL" -ForegroundColor Red
            $Fail++
        }
        elseif ($result -match 'RESULT: (BOOT_FAILURE|UNEXPECTED_EXIT|TIMEOUT|INFRASTRUCTURE_ERROR)') {
            $reason = $matches[1]
            Write-Host "$reason" -ForegroundColor Red
            $Fail++
        }
        else {
            Write-Host "UNKNOWN RESULT" -ForegroundColor Red
            Write-Host $result
            $Fail++
        }
    }
    catch {
        Write-Host "ERROR: $_" -ForegroundColor Red
        $Fail++
    }
}

Write-Host ""
Write-Host "Results: $Pass/$Tries passed, $Fail failed" -ForegroundColor $(if ($Fail -eq 0) { "Green" } else { "Red" })

if ($PanicCycles.Count -gt 0) {
    Write-Host "Panic cycles: $($PanicCycles -join ', ')" -ForegroundColor Red
}

if ($Fail -gt 0) { exit 1 }
