#!/usr/bin/env python3
"""T-00 automated QEMU self-test runner for the Vahi kernel.

Builds nothing: run after building the kernel with --features self_test and
creating a bootimage (e.g. via builder/build_limine_image.py). Boots the image
in QEMU with deterministic serial capture, parses the TAP 13 protocol emitted
by kernel/src/selftest.rs, classifies the result, and exits:

    0  PASS                 - suite ran to completion, all tests passed
    1  FAIL                 - one or more TAP tests reported "not ok"
    2  PANIC                - kernel panic (Bail out! / KERNEL PANIC marker)
    3  TIMEOUT              - no verdict within --timeout seconds
    4  BOOT_FAILURE         - QEMU died before any test output appeared
    5  UNEXPECTED_EXIT      - QEMU exited mid-suite without a verdict
    6  INFRASTRUCTURE_ERROR - missing image/binary or broken runner inputs

Success is never inferred from the QEMU process exiting; the kernel must emit
an explicit TAP summary, and the isa-debug-exit device turns a completed suite
into an explicit QEMU exit status (0x10 -> exit 15) as a second signal.

Usage:
    python tests/run_qemu_tests.py --image bootimage-vahi_kernel.bin
    python tests/run_qemu_tests.py --image tests/t00_selftest.bin --timeout 900

Windows note: serial goes to a file (-serial file:...) because QEMU's Windows
stdio is not a byte pipe; that keeps capture identical across hosts.
"""

from __future__ import annotations

import argparse
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

# Result classification (process exit codes, documented above).
PASS, FAIL, PANIC, TIMEOUT, BOOT_FAILURE, UNEXPECTED_EXIT, INFRA_ERROR = range(7)

LABEL = {
    PASS: "PASS",
    FAIL: "FAIL",
    PANIC: "PANIC",
    TIMEOUT: "TIMEOUT",
    BOOT_FAILURE: "BOOT_FAILURE",
    UNEXPECTED_EXIT: "UNEXPECTED_EXIT",
    INFRA_ERROR: "INFRASTRUCTURE_ERROR",
}

# isa-debug-exit: QEMU exits with (value & 0x7f) - 1. See kernel/src/selftest.rs
# for the kernel-side constants these encode.
QEMU_EXIT_PASS = 0x10 - 1    # 15: suite completed (verdict comes from TAP)
QEMU_EXIT_PANIC = 0x11 - 1   # 16: panic handler fired

ANSI_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
TAP_PLAN_RE = re.compile(r"^1\.\.(\d+)")
TAP_OK_RE = re.compile(r"^(ok|not ok) (\d+) - (\S+)(?:\s*#\s*(.*))?$")
TAP_SUMMARY_RE = re.compile(r"^# (\d+)/(\d+) passed, (\d+) failed")
BOOT_MARKERS = ("[SELFTEST] T-00 test mode active", "limine:")


def strip_ansi(data: bytes) -> str:
    return ANSI_RE.sub("", data.decode("utf-8", errors="replace"))


def find_qemu(explicit: str | None) -> str:
    if explicit:
        return explicit
    found = shutil.which("qemu-system-x86_64")
    if found:
        return found
    for candidate in (r"C:\Program Files\qemu\qemu-system-x86_64.exe",):
        if os.path.isfile(candidate):
            return candidate
    return ""


def find_ovmf(root: str, explicit: str | None) -> str:
    if explicit:
        return explicit
    for name in ("OVMF.fd", "OVMF_CODE.fd"):
        candidate = os.path.join(root, name)
        if os.path.isfile(candidate):
            return candidate
    return ""


def run(image: str, qemu: str, ovmf: str, timeout_s: int, smp: int,
        mem: str, log_path: str, serial_path: str, boot_wait_s: int,
        boot_only: bool, expect_ap: int, qemu_extra: list[str]) -> tuple[int, str]:
    """Boot the image, stream serial to disk, classify the outcome.

    Returns (classification, human-readable detail)."""
    cmd = [qemu,
           "-m", mem,
           "-smp", str(smp),
           "-drive", f"format=raw,file={image}",
           "-serial", f"file:{serial_path}",
           "-display", "none",
           "-no-reboot",
           "-accel", "tcg",
           "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]
    if ovmf:
        # UEFI boot via OVMF. readonly: the guest must never rewrite firmware.
        cmd += ["-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}"]
    cmd += qemu_extra

    with open(log_path, "w", encoding="utf-8") as log:
        log.write("# command: " + " ".join(cmd) + "\n")
        log.flush()
        proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL,
                                stderr=subprocess.STDOUT,
                                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        deadline = time.monotonic() + timeout_s
        boot_deadline = time.monotonic() + boot_wait_s
        summary = None
        failed_lines: list[str] = []
        panic = False
        saw_boot = False
        saw_plan = False
        tap_total = None
        boot_progress = False
        poll = 0.5
        try:
            while True:
                exited = proc.poll() is not None
                try:
                    raw = open(serial_path, "rb").read()
                except OSError:
                    raw = b""
                text = strip_ansi(raw)
                if not saw_boot and any(m in text for m in BOOT_MARKERS):
                    saw_boot = True
                if boot_only and "[BOOT] VFS init" in text:
                    # Normal-boot check: VFS init is deep boot progress that
                    # requires memory, heap, graphics, GDT/IDT, APIC, PCI and
                    # devices to have survived — a strict regression signal.
                    if expect_ap > 0:
                        # K-02 SMP regression: additionally require that the
                        # requested number of application processors reported
                        # ``[AP] count incremented`` before VFS init. If an AP
                        # triple-faults, the BSP dies with it, so absence of
                        # AP markers on an -smp N+1 boot is the failure signal.
                        aps = text.count("[AP] count incremented")
                        if aps < expect_ap:
                            proc.kill()
                            return BOOT_FAILURE, (
                                f"SMP regression: only {aps}/{expect_ap} APs "
                                "reported '[AP] count incremented' before "
                                "[BOOT] VFS init")
                        proc.kill()
                        return PASS, (
                            f"SMP boot verified: {aps}/{expect_ap} AP(s) up + "
                            "[BOOT] VFS init reached")
                    proc.kill()
                    return PASS, "boot progress verified: [BOOT] VFS init reached"
                lines = text.splitlines()
                for line in lines:
                    if TAP_PLAN_RE.match(line):
                        saw_plan = True
                        m = TAP_PLAN_RE.match(line)
                        assert m is not None
                        tap_total = int(m.group(1))
                    m = TAP_SUMMARY_RE.match(line)
                    if m:
                        summary = (int(m.group(2)), int(m.group(1)), int(m.group(3)))
                    if "KERNEL PANIC" in line or line.startswith("Bail out!"):
                        panic = True
                    mok = TAP_OK_RE.match(line)
                    if mok and mok.group(1) == "not ok" and line not in failed_lines:
                        # Serial is re-read whole each poll; dedupe so each
                        # failing test is reported once.
                        failed_lines.append(line)
                if panic and summary is None:
                    # Panic marker present without a completed summary.
                    code = PANIC
                    detail = "kernel panic marker observed before suite summary"
                    if failed_lines:
                        detail += "; failing tests: " + "; ".join(failed_lines[:5])
                    return code, detail
                if summary is not None:
                    total, passed, failed = summary
                    if failed == 0 and passed == total and total > 0 and tap_total == total:
                        return PASS, f"TAP summary: {passed}/{total} passed, 0 failed"
                    detail = f"TAP summary: {passed}/{total} passed, {failed} failed"
                    if failed_lines:
                        detail += "\n  failing tests:\n    " + "\n    ".join(failed_lines[:20])
                    return FAIL, detail
                if exited:
                    rc = proc.returncode
                    if rc == QEMU_EXIT_PASS:
                        return UNEXPECTED_EXIT, (
                            "QEMU signalled suite completion (isa-debug-exit 0x10) "
                            "but no TAP summary was parsed — protocol desync")
                    if rc == QEMU_EXIT_PANIC:
                        return PANIC, "QEMU panic exit code (isa-debug-exit 0x11)"
                    if not saw_boot:
                        return BOOT_FAILURE, f"QEMU exited rc={rc} before any boot output"
                    return UNEXPECTED_EXIT, (
                        f"QEMU exited rc={rc} mid-suite "
                        f"(plan={tap_total}, summary not reached)")
                now = time.monotonic()
                if not saw_boot and now > boot_deadline:
                    proc.kill()
                    return BOOT_FAILURE, (
                        f"no boot output within {boot_wait_s}s "
                        "(wrong image, or bootloader never started)")
                if now > deadline:
                    proc.kill()
                    detail = f"no verdict within {timeout_s}s"
                    if tap_total is not None:
                        done = len([ln for ln in lines if TAP_OK_RE.match(ln)])
                        detail += f" (plan={tap_total}, observed={done} results)"
                    return TIMEOUT, detail
                time.sleep(poll)
        finally:
            if proc.poll() is None:
                proc.kill()
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    pass


def main() -> int:
    ap = argparse.ArgumentParser(description="T-00 QEMU self-test runner")
    ap.add_argument("--image", required=True, help="bootimage .bin path")
    ap.add_argument("--qemu", default=None, help="qemu-system-x86_64 binary")
    ap.add_argument("--ovmf", default=None, help="OVMF firmware file (default: repo OVMF.fd)")
    ap.add_argument("--timeout", type=int, default=1200,
                    help="wall-clock budget for the whole boot+suite (seconds)")
    ap.add_argument("--boot-wait", type=int, default=120,
                    help="seconds to wait for first boot output before BOOT_FAILURE")
    ap.add_argument("--smp", type=int, default=1)
    ap.add_argument("--mem", default="512M")
    ap.add_argument("--log", default=None, help="where to write the runner log")
    ap.add_argument("--serial", default=None, help="serial capture file (default: temp)")
    ap.add_argument("--boot-only", action="store_true",
                    help="normal-boot regression check: PASS when deep boot "
                         "progress is reached; no self_test/TAP expected")
    ap.add_argument("--expect-ap", type=int, default=0,
                    help="with --boot-only: require N '[AP] count incremented' "
                         "markers (SMP regression, K-02)")
    ap.add_argument("--qemu-extra", default="",
                    help="extra QEMU arguments as one quoted string "
                         "(e.g. \"-vga none\")")
    args = ap.parse_args()

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    image = args.image if os.path.isabs(args.image) else os.path.join(os.getcwd(), args.image)
    qemu = find_qemu(args.qemu)
    if not qemu:
        print("INFRASTRUCTURE_ERROR: qemu-system-x86_64 not found (use --qemu)")
        return INFRA_ERROR
    if not os.path.isfile(image):
        print(f"INFRASTRUCTURE_ERROR: image not found: {image}")
        return INFRA_ERROR
    ovmf = find_ovmf(root, args.ovmf)

    serial_path = args.serial or os.path.join(tempfile.gettempdir(), "vahi_t00_serial.log")
    log_path = args.log or os.path.join("tests", "t00_last_run.log")
    for p in (serial_path,):
        try:
            os.remove(p)
        except OSError:
            pass

    print(f"=== T-00 QEMU self-test runner ===")
    print(f"image : {image}")
    print(f"qemu  : {qemu}")
    print(f"ovmf  : {ovmf or '(BIOS fallback)'}")
    print(f"smp   : {args.smp}  mem: {args.mem}  timeout: {args.timeout}s")

    t0 = time.monotonic()
    try:
        code, detail = run(image, qemu, ovmf, args.timeout, args.smp, args.mem,
                           log_path, serial_path, args.boot_wait, args.boot_only,
                           args.expect_ap, shlex.split(args.qemu_extra))
    except OSError as exc:
        code, detail = INFRA_ERROR, f"runner I/O failure: {exc}"

    elapsed = time.monotonic() - t0
    print(f"\nRESULT: {LABEL[code]}  ({elapsed:.1f}s)")
    print(detail)
    try:
        with open(log_path, "a", encoding="utf-8") as log:
            log.write(f"RESULT: {LABEL[code]}\n{detail}\n")
    except OSError:
        pass
    return code


if __name__ == "__main__":
    sys.exit(main())
