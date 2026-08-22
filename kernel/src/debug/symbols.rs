use crate::println;
use x86_64::VirtAddr;

pub fn print_stack_trace() {
    println!("Call Stack:");
    let mut curr_rbp: *const usize;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) curr_rbp);
    }

    // Kernel image and thread stacks live in [0xFFFF_8000_0000_0000,
    // 0xFFFF_E000_0000_0000). A panic from a user-mode fault (or after
    // corruption) can leave RBP pointing at a USER address or garbage —
    // walking it faulted again and masked the real panic. Require kernel
    // range + 8-byte alignment, and cap the walk.
    let kbase: usize = 0xFFFF_8000_0000_0000;
    let ktop: usize = 0xFFFF_E000_0000_0000;
    let mut depth = 0usize;
    while !curr_rbp.is_null()
        && (curr_rbp as usize) >= kbase
        && (curr_rbp as usize) < ktop
        && (curr_rbp as usize) & 0x7 == 0
        && depth < 40
    {
        depth += 1;
        let ret_addr = unsafe { *curr_rbp.offset(1) };
        if ret_addr == 0 { break; }

        let symbol = lookup_symbol(VirtAddr::new(ret_addr as u64));
        println!("  [{:016x}] {}", ret_addr, symbol);

        curr_rbp = unsafe { *curr_rbp as *const usize };
    }
}

pub fn lookup_symbol(_addr: VirtAddr) -> &'static str {
    // In a real implementation, we'd parse the ELF symbol table or 
    // a pre-generated symbol file. For now, we return a stub.
    "<unknown symbol>"
}
