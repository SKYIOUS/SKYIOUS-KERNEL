# SkyOS Panic Recovery Test — trigger kernel panic, verify reboot
$repo = "C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL"
$qemu = "qemu-system-x86_64"
$bios = "$env:USERPROFILE\.cargo\bootimage\OVMF-pure-efi.fd"

Write-Host "=== SkyOS Panic Recovery Test ==="

$result = & $qemu -bios $bios -cpu max -smp 1 -m 512M -no-reboot -nographic `
  -drive format=raw,file="$repo\target\x86_64-vahi\debug\bootimage-vahi_kernel.bin" `
  -serial stdio -nic user -k en-us -rtc base=localtime `
  -append "panic=1" 2>&1

$ok = $result -match "PANIC"
if ($ok) { Write-Host "PASS: Kernel panicked as expected" -ForegroundColor Green }
else { Write-Host "FAIL: No panic triggered" -ForegroundColor Red }
exit $(if ($ok) { 0 } else { 1 })
