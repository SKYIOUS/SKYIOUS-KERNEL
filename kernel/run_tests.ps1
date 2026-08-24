# PowerShell script to run cargo tests with reduced resource usage.
# Limits parallel compilation and test execution to prevent system unresponsiveness.
#
# Usage:
#   .\run_tests.ps1                    # Run all tests
#   .\run_tests.ps1 -Filter <pattern>  # Run tests matching filter
#   .\run_tests.ps1 -Lib               # Run only library unit tests (skip integration tests)

param(
    [string]$Filter = "",
    [switch]$Lib,
    [switch]$NoRun
)

$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot
Set-Location $rootDir

Write-Host "--- Running tests with limited resources ---" -ForegroundColor Cyan
Write-Host "CARGO_BUILD_JOBS=1 (sequential compilation)" -ForegroundColor Gray
Write-Host "--test-threads=1 (sequential test execution)" -ForegroundColor Gray

$env:CARGO_BUILD_JOBS = "1"

$args = @("test")

if ($Lib) {
    $args += "--lib"
}

if ($Filter) {
    $args += $Filter
}

if ($NoRun) {
    $args += "--no-run"
}

$args += "--test-threads=1"

& cargo @args
