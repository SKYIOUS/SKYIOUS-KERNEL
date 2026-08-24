#[cfg(target_arch = "x86_64")]
use x86_64::VirtAddr;

pub fn print_stack_trace() {
    crate::serial_write("Call Stack:\n");

    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: reading frame pointer in panic context — no side effects.
        let mut rbp: *const usize;
        unsafe {
            core::arch::asm!("mov {}, rbp", out(reg) rbp);
        }
        walk_x86_64_stack(rbp);
    }

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: reading frame pointer register in panic context.
        let mut fp: *const usize;
        unsafe {
            core::arch::asm!("mov {}, x29", out(reg) fp);
        }
        walk_aarch64_stack(fp);
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        crate::serial_write("  <stack trace not supported on this architecture>\n");
    }
}

/// x86_64: Walk the RBP (frame pointer) chain.
#[cfg(target_arch = "x86_64")]
fn walk_x86_64_stack(mut rbp: *const usize) {
    // Kernel image and thread stacks live in [0xFFFF_8000_0000_0000,
    // 0xFFFF_E000_0000_0000). Reject pointers outside that range or
    // with wrong alignment — a corrupted RBP would fault again.
    const KBASE: usize = 0xFFFF_8000_0000_0000;
    const KTOP: usize = 0xFFFF_E000_0000_0000;
    let mut depth = 0usize;
    while !rbp.is_null()
        && (rbp as usize) >= KBASE
        && (rbp as usize) < KTOP
        && (rbp as usize) & 0x7 == 0
        && depth < 40
    {
        depth += 1;
        // SAFETY: we validated rbp is within kernel range and aligned.
        let ret_addr = unsafe { *rbp.offset(1) };
        if ret_addr == 0 {
            break;
        }

        let symbol = lookup_symbol(VirtAddr::new(ret_addr as u64));
        crate::serial_write(&alloc::format!("  [{:016x}] {}\n", ret_addr, symbol));

        // SAFETY: validated rbp is non-null and within kernel range.
        rbp = unsafe { *rbp as *const usize };
    }
}

/// aarch64: Walk the FP (x29) chain. The layout is:
///   [FP] -> previous FP
///   [FP + 8] -> LR (return address)
#[cfg(target_arch = "aarch64")]
fn walk_aarch64_stack(mut fp: *const usize) {
    const KBASE: usize = 0xFFFF_0000_0000_0000;
    const KTOP: usize = 0xFFFF_FFFF_FFFF_FFFF;
    let mut depth = 0usize;
    while !fp.is_null()
        && (fp as usize) >= KBASE
        && (fp as usize) < KTOP
        && (fp as usize) & 0x7 == 0
        && depth < 40
    {
        depth += 1;
        let lr = unsafe { *fp.offset(1) };
        if lr == 0 {
            break;
        }
        crate::serial_write(&alloc::format!("  [{:016x}] <unknown>\n", lr));
        fp = unsafe { *fp as *const usize };
    }
}

#[cfg(target_arch = "x86_64")]
pub fn lookup_symbol(_addr: VirtAddr) -> &'static str {
    "<unknown symbol>"
}
