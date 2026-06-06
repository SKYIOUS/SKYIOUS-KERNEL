# Build userspace binaries for SkyOS

$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot

Write-Host "--- Building SkyOS Userspace ---" -ForegroundColor Cyan

# Step 1: Build with cargo
Set-Location "$rootDir\userspace"
$target = "x86_64-skyos"
$targetJson = "target\$target.json"

# Build release for smaller binaries
$env:RUSTC_BOOTSTRAP=1
cargo build --target "$targetJson" --release -Z "build-std=core,alloc" -Z "build-std-features=compiler-builtins-mem" 2>&1

if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Userspace build failed!" -ForegroundColor Red
    exit 1
}

# Step 2: Create initrd directory structure
$initrdDir = New-Item -ItemType Directory -Path "$rootDir\SkyOS" -Force
$binDir = New-Item -ItemType Directory -Path "$rootDir\SkyOS\bin" -Force

# Copy binaries
$releaseDir = "target\$target\release"
Copy-Item "$releaseDir\init" "$binDir\init" -Force
Copy-Item "$releaseDir\sargash" "$binDir\sargash" -Force
Copy-Item "$releaseDir\svc" "$binDir\svc" -Force
Copy-Item "$releaseDir\vahid" "$binDir\vahid" -Force
# Coreutils
@('ls','cat','mkdir','rm','cp','mv','ps','clear','uname','printenv','sleep','yes',
  'rmdir','touch','hostname','which','env','echo','head','tail','wc','grep','ln','chmod',
  'printf','sort','uniq','uptime',
  'ping','nslookup','wget','ifconfig','netstat','telnet',
  'beep','dd','blkid','fdisk','df','du') | ForEach-Object {
    Copy-Item "$releaseDir\$_" "$binDir\$_" -Force
}

# Rename mkfs_* to mkfs.* (traditional naming)
Copy-Item "$releaseDir\mkfs_ext2" "$binDir\mkfs.ext2" -Force
Copy-Item "$releaseDir\mkfs_fat" "$binDir\mkfs.fat" -Force
# SkyEdit
Copy-Item "$releaseDir\skyedit" "$binDir\skyedit" -Force
# Sarga Display Server
Copy-Item "$releaseDir\sarga-disp" "$binDir\sarga-disp" -Force
# Package Manager
Copy-Item "$releaseDir\skypkg" "$binDir\skypkg" -Force
# Security utilities
Copy-Item "$releaseDir\login" "$binDir\login" -Force
Copy-Item "$releaseDir\passwd" "$binDir\passwd" -Force
# Developer toolchain
Copy-Item "$releaseDir\skybuild" "$binDir\skybuild" -Force
Copy-Item "$releaseDir\setup" "$binDir\setup" -Force

# Step 3: Create initrd.tar with Python (FHS-aware build)
python "$rootDir\build_initrd.py" "$rootDir\SkyOS"

if ($LASTEXITCODE -ne 0) {
    Write-Host "WARNING: Python tar creation failed, trying manual copy..." -ForegroundColor Yellow
}

Set-Location $rootDir
Write-Host "SUCCESS: Userspace built and initrd.tar created." -ForegroundColor Green
