$ErrorActionPreference = "Stop"

# The SkyOS directory is at C:\Users\nanda\Desktop\Github\SkyOS
# The SKYIOUS KERNEL directory is at C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL
$skyosDir = "C:\Users\nanda\Desktop\Github\SkyOS"
$kernelDir = "C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL"

# Find the built init binary
$newInit = Get-ChildItem -Recurse -Filter "init" -Path "$skyosDir" | Where-Object { $_.Length -eq 18632 } | Select-Object -First 1
if (!$newInit) {
    Write-Host "ERROR: New init binary not found!" -ForegroundColor Red
    exit 1
}
Write-Host "Found new init: $($newInit.FullName) ($($newInit.Length) bytes)"

# Copy to SkyOS/bin
Copy-Item $newInit.FullName "$kernelDir\SkyOS\bin\init" -Force
Write-Host "Copied to $kernelDir\SkyOS\bin\init"

# Rebuild initrd
python "$kernelDir\build_initrd.py" "$kernelDir\SkyOS"
Write-Host "initrd rebuilt"
