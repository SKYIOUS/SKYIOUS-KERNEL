$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot

Write-Host "--- Building SARGA OS Userspace ---" -ForegroundColor Cyan

# The userspace source lives in the SkyOS repo (sibling directory)
$skyosDir = Join-Path $rootDir "..\SkyOS"
if (Test-Path $skyosDir) {
    Write-Host "Building userspace from $skyosDir ..." -ForegroundColor Gray
    Set-Location $skyosDir
    & .\build.ps1 all
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Userspace build failed!" -ForegroundColor Red
        Set-Location $rootDir
        exit 1
    }
    Set-Location $rootDir
} else {
    Write-Host "SkyOS userspace repo not found at $skyosDir" -ForegroundColor Yellow
    Write-Host "Using existing SkyOS/initrd.tar (if present)" -ForegroundColor Yellow
}
