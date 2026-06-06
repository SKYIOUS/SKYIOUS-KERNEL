# SkyOS Login Test — send username+password, expect shell prompt
$repo = "C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL"
$qemu = "qemu-system-x86_64"
$bios = "$env:USERPROFILE\.cargo\bootimage\OVMF-pure-efi.fd"

Write-Host "=== SkyOS Login Test ==="

$script = @"
set timeout 30
spawn $qemu -bios $bios -cpu max -smp 1 -m 512M -no-reboot -nographic `
  -drive format=raw,file="$repo\target\x86_64-vahi\debug\bootimage-vahi_kernel.bin" `
  -serial stdio -nic user -k en-us -rtc base=localtime

expect {
    "login:" { send "root\r"; exp_continue }
    "Password:" { send "root\r"; exp_continue }
    "\$ " { puts "PASS: Got shell prompt"; exit 0 }
    timeout { puts "FAIL: Timeout"; exit 1 }
    eof { puts "FAIL: QEMU exited early"; exit 1 }
}
"@

$tmp = [System.IO.Path]::GetTempFileName()
$script | Out-File -Encoding utf8 "$tmp.exp"
$result = & "expect" "$tmp.exp" 2>&1
Write-Host $result
Remove-Item "$tmp.exp" -Force
