# boot_stress_100.ps1 — Run 100 consecutive QEMU boots, report pass/fail.
# Usage: .\tests\boot_stress_100.ps1 [-Timeout 90] [-Smp 1]

param(
    [int]$Timeout = 90,
    [int]$Smp = 1,
    [int]$Tries = 100
)

$Pass = 0
$Fail = 0
$FailTokens = @("not ok", "Bail out!", "KERNEL PANIC", "Panicked")
$PassToken = "starting service"
$Qemu = "qemu-system-x86_64"
$Drive = "bootimage-vahi_kernel.bin"

if (-not (Test-Path $Drive)) {
    Write-Host "ERROR: $Drive not found. Run 'make boot' first." -ForegroundColor Red
    exit 1
}

Write-Host "Boot stress test: $Tries tries, SMP=$Smp, timeout=${Timeout}s" -ForegroundColor Cyan

for ($i = 1; $i -le $Tries; $i++) {
    $Log = "$env:TEMP\boot_stress_$i.log"
    $args = @(
        "-drive", "file=$Drive,format=raw",
        "-m", "512M",
        "-smp", "$Smp",
        "-serial", "file:$Log",
        "-display", "none",
        "-no-reboot",
        "-accel", "tcg"
    )

    $proc = Start-Process -FilePath $Qemu -ArgumentList $args -PassThru -NoNewWindow
    $exited = $proc.WaitForExit($Timeout * 1000)

    if (-not $exited) {
        $proc.Kill()
        Write-Host "  [$i/$Tries] TIMEOUT" -ForegroundColor Yellow
        $Fail++
        continue
    }

    $output = if (Test-Path $Log) { Get-Content $Log -Raw } else { "" }

    $failed = $false
    foreach ($token in $FailTokens) {
        if ($output -match [regex]::Escape($token)) {
            $failed = $true
            break
        }
    }

    if ($failed) {
        Write-Host "  [$i/$Tries] FAIL" -ForegroundColor Red
        $Fail++
    } else {
        Write-Host "  [$i/$Tries] PASS" -ForegroundColor Green
        $Pass++
    }

    Remove-Item $Log -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "Results: $Pass/$Tries passed, $Fail failed" -ForegroundColor $(if ($Fail -eq 0) { "Green" } else { "Red" })

if ($Fail -gt 0) { exit 1 }
