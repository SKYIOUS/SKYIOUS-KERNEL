# boot_stress_100_fixed.ps1 — Run 100 consecutive QEMU boots via the Python runner.
# This version uses direct command invocation with & operator to avoid script execution context issues.

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

    try {
        # Use direct command invocation with & operator to avoid script context issues
        $cwd = Get-Location
        Write-Host "DEBUG: cwd=$cwd" -ForegroundColor Gray
        $result = & "C:\Windows\py.exe" -3 tests/run_qemu_tests.py --image $Image --timeout $Timeout --smp $Smp --qemu-extra "-vga none" 2>&1
        $exitCode = $LASTEXITCODE
        Write-Host "DEBUG: type=$($result.GetType().FullName), count=$($result.Count), exitCode=$exitCode" -ForegroundColor Gray
        Write-Host "DEBUG: first line=$($result[0])" -ForegroundColor Gray

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
            $reason = if ($matches -and $matches.Count -gt 1) { $matches[1] } else { "UNKNOWN" }
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

    # Wait between runs to let QEMU fully clean up
    Start-Sleep 3
}

Write-Host ""
Write-Host "Results: $Pass/$Tries passed, $Fail failed" -ForegroundColor $(if ($Fail -eq 0) { "Green" } else { "Red" })

if ($PanicCycles.Count -gt 0) {
    Write-Host "Panic cycles: $($PanicCycles -join ', ')" -ForegroundColor Red
}

if ($Fail -gt 0) { exit 1 }