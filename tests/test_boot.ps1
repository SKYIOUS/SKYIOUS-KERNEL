# SkyOS Boot Test — nographic serial-only
$bootimg = "C:\Users\nanda\bootimg.bin"
$qemu = "qemu-system-x86_64"
$bios = "C:\Program Files\qemu\OVMF.fd"

Write-Host "=== SkyOS Boot Test ==="
$outFile = Join-Path $env:TEMP "boot_test.txt"

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $qemu
$psi.Arguments = "-bios `"$bios`" -cpu max -smp 1 -m 512M -no-reboot -nographic -drive format=raw,file=`"$bootimg`" -serial stdio -nic user -k en-us -rtc base=localtime"
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.CreateNoWindow = $true
$p = [System.Diagnostics.Process]::Start($psi)

$timeout = 30; $elapsed = 0
while (-not $p.HasExited -and $elapsed -lt $timeout) { Start-Sleep -Seconds 1; $elapsed++ }
if (-not $p.HasExited) { $p.Kill(); Write-Host "TIMEOUT" }
$out = $p.StandardOutput.ReadToEnd()
$out | Out-File $outFile -Encoding utf8

if ($out -match "login:") { Write-Host "PASS" -ForegroundColor Green; exit 0 }
else { Write-Host "FAIL"; $out -split "`n" | Select-String -Pattern "init|login|panic|BOOT|SPLASH" | Select-Object -First 10; exit 1 }
