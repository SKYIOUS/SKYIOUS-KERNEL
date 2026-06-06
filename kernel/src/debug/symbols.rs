use crate::println;
use x86_64::VirtAddr;

pub fn print_stack_trace() {
    println!("Call Stack:");
    let mut curr_rbp: *const usize;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) curr_rbp);
    }

    while !curr_rbp.is_null() && (curr_rbp as usize) < 0xFFFF_E000_0000_0000 {
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
