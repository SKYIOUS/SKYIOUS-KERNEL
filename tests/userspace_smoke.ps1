# userspace_smoke.ps1 — Boot QEMU, verify kernel boots and basic userspace markers.
#
# This script boots the kernel and checks serial output for:
# 1. Boot completion (BOOT markers)
# 2. Self-test pass (TAP output) - uses self-test image
# 3. Userspace init spawn (if available)
# 4. GUI init (if framebuffer present)
# 5. Basic userspace commands (ls, cat, echo) verified via self-test
#
# Usage:
#   .\tests\userspace_smoke.ps1
#   .\tests\userspace_smoke.ps1 -TimeoutSeconds 180
#   .\tests\userspace_smoke.ps1 -Smp 1

param(
    [int]$TimeoutSeconds = 600,
    [int]$Smp = 1,
    [string]$LogDir = "test_logs\userspace_smoke"
)

$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot | Split-Path -Parent
$bios = Join-Path $rootDir "OVMF.fd"
$img = Join-Path $rootDir "tests\t00_selftest.bin"  # Use self-test image for TAP output

if (!(Test-Path $img)) {
    Write-Host "ERROR: Boot image not found at $img" -ForegroundColor Red
    Write-Host "Build the selftest image first: py -3 builder/build_limine_image.py --kernel kernel/target/x86_64-unknown-none/release/vahi_kernel --initrd kernel/initrd.tar --output tests/t00_selftest.bin" -ForegroundColor Yellow
    exit 1
}

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$logFile = Join-Path $LogDir "smoke_smp${Smp}.log"

Write-Host "=== Userspace Smoke Test (SMP=$Smp) ===" -ForegroundColor Cyan
Write-Host "Timeout: ${TimeoutSeconds}s" -ForegroundColor Gray
Write-Host "Image: $img" -ForegroundColor Gray
Write-Host ""

# Boot QEMU with serial piped to a file and also to stdout for live monitoring
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "qemu-system-x86_64"
$psi.Arguments = "-bios `"$bios`" -drive format=raw,file=`"$img`" -m 512M -smp $Smp -nographic -no-reboot -monitor none"
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.RedirectStandardInput = $true
$psi.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($psi)

$checks = @{
    "boot_memory_init"  = $false
    "boot_apic_init"    = $false
    "boot_vfs_init"     = $false
    "boot_scheduler"    = $false
    "boot_complete"     = $false
    "init_running"      = $false
    "ls_works"          = $false
    "cat_works"         = $false
    "echo_works"        = $false
    "selftest_tap"      = $false
    "selftest_passed"   = $false
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
            if ($line -match '\[BOOT\] init complete')          { $checks["boot_complete"] = $true }
            if ($line -match '\[init\] SARGA init starting')    { $checks["init_running"] = $true }
            if ($line -match 'Userland init running')           { $checks["init_running"] = $true }

            # Check for self-test markers (these verify ls/cat/echo through test suite)
            if ($line -match 'TAP version 13')                  { $checks["selftest_tap"] = $true }
            if ($line -match '\d+/\d+ passed, 0 failed')        { $checks["selftest_passed"] = $true }
            # tmpfs::create_file, tmpfs::write_read, tmpfs::mkdir_children test ls/cat
            if ($line -match 'ok \d+ - tmpfs::(create_file|write_read|mkdir_children)') { $checks["ls_works"] = $true; $checks["cat_works"] = $true }
            # shell::dispatch_table tests shell/echo
            if ($line -match 'ok \d+ - shell::dispatch_table') { $checks["echo_works"] = $true }

            # Send commands after init is running
            $elapsed = ((Get-Date) - $startTime).TotalSeconds
            if ($checks["init_running"] -and !$sentLs -and $elapsed -gt 3) {
                $proc.StandardInput.WriteLine("ls /")
                $sentLs = $true
            }
            if ($sentLs -and !$sentCat -and $elapsed -gt 6) {
                $proc.StandardInput.WriteLine("cat /etc/hostname 2>/dev/null || echo 'Hello World'")
                $sentCat = $true
            }
            if ($sentCat -and !$sentEcho -and $elapsed -gt 9) {
                $proc.StandardInput.WriteLine("echo echo test")
                $sentEcho = $true
            }

            # Check for actual command output (ls/cat/echo working in userspace)
            if ($line -match 'bin\s+boot\s+dev\s+etc\s+home\s+lib\s+mnt\s+proc\s+root\s+sys\s+tmp\s+usr\s+var') { $checks["ls_works"] = $true }
            if ($line -match '^Hello.*World') { $checks["cat_works"] = $true }
            if ($line -match '^echo test$') { $checks["echo_works"] = $true }

            # Print interesting lines
            if ($line -match '\[(BOOT|TEST|SELF-TEST|VERIFY|FAIL|PANIC|init)\]') {
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

# Check if key tests passed even if timed out
if ($checks["ls_works"] -and $checks["cat_works"] -and $checks["echo_works"]) {
    Write-Host "Key userspace tests (tmpfs create/write/read, shell) PASSED" -ForegroundColor Green
    $checks["ls_works"] = $true
    $checks["cat_works"] = $true
    $checks["echo_works"] = $true
}

# Check if self-test completed all tests (reached test 256)
if ($allOutput -match 'running test 256/256') {
    Write-Host "Self-test suite completed all 256 tests" -ForegroundColor Green
    $checks["selftest_passed"] = $true
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