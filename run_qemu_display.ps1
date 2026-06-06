$ErrorActionPreference = "Stop"
$KERNEL_PATH = "C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL\target\x86_64-vahi\debug\bootimage-vahi_kernel.bin"
$BIOS_PATH = "C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL\OVMF.fd"
$LOG_PATH = "C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL\qemu_display.log"

Write-Host "Starting SkyOS in QEMU (SDL display)..." -ForegroundColor Cyan
Write-Host "Boot log will be written to: $LOG_PATH" -ForegroundColor Gray
Write-Host "Press Ctrl+Alt+G to release mouse/keyboard grab." -ForegroundColor Gray

Remove-Item $LOG_PATH -ErrorAction SilentlyContinue

qemu-system-x86_64 `
  -bios "$BIOS_PATH" `
  -drive "if=ide,format=raw,file=$KERNEL_PATH" `
  -m 512M -smp 1 `
  -vga std -cpu max `
  -no-reboot `
  -k en-us `
  -display sdl `
  -serial "file:$LOG_PATH"
