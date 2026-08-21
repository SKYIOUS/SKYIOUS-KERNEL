use crate::selftest;
use super::pata_read_test;

pub fn test_entropy() -> Result<(), &'static str> {
    let e1 = crate::crypto::GLOBAL_ENTROPY.get_u64();
    let e2 = crate::crypto::GLOBAL_ENTROPY.get_u64();
    if e1 == e2 {
        return Err("Entropy harvester returned duplicate values");
    }
    if e1 == 0 && e2 == 0 {
        return Err("Entropy harvester returned all zeros");
    }
    Ok(())
}

pub fn test_page_cache() -> Result<(), &'static str> {
    use crate::vfs::page_cache::GLOBAL_PAGE_CACHE;
    let ino = 9999;
    let data = [0xAAu8; 4096];
    GLOBAL_PAGE_CACHE.insert_page(ino, 0, data);

    let cached = GLOBAL_PAGE_CACHE.get_page(ino, 0).ok_or("Page not found in cache")?;
    if cached.lock().data[0] != 0xAA {
        return Err("Cached data mismatch");
    }

    GLOBAL_PAGE_CACHE.evict_inode(ino);
    if GLOBAL_PAGE_CACHE.get_page(ino, 0).is_some() {
        return Err("Page still in cache after eviction");
    }

    Ok(())
}

pub fn test_gdt_selectors() -> Result<(), &'static str> {
    let sel = crate::gdt::get_selectors();
    if sel.code_selector.0 == 0 {
        return Err("GDT code_selector is null");
    }
    if sel.data_selector.0 == 0 {
        return Err("GDT data_selector is null");
    }
    if sel.user_code_selector.0 == 0 {
        return Err("GDT user_code_selector is null");
    }
    if sel.user_data_selector.0 == 0 {
        return Err("GDT user_data_selector is null");
    }
    if sel.tss_selector.0 == 0 {
        return Err("GDT tss_selector is null");
    }
    Ok(())
}

pub fn test_tss_stack() -> Result<(), &'static str> {
    let stack = crate::gdt::get_kernel_stack();
    if stack.as_u64() == 0 {
        return Err("TSS privilege stack is zero");
    }
    Ok(())
}

pub fn test_ticks() -> Result<(), &'static str> {
    let t1 = crate::interrupts::get_ticks();
    for _ in 0..100_000 { core::hint::spin_loop(); }
    let t2 = crate::interrupts::get_ticks();
    if t2 < t1 {
        return Err("Ticks decreased");
    }
    Ok(())
}

pub fn test_phys_alloc() -> Result<(), &'static str> {
    crate::memory::phys::test_alloc_free()
}

pub fn test_user_copy_fault_abort() -> Result<(), &'static str> {
    use crate::syscalls::user_access::{copy_from_user, copy_to_user, user_copy_active};
    // Unmapped user-range address (4 GiB; phys memory lives at 0xFFFF_8000_...).
    // The fault must abort the copy to Err(()) without panicking the kernel.
    let bad: *const u8 = 0x0000_1000_0000 as *const u8;
    let mut buf = [0u8; 64];
    if unsafe { copy_from_user(&mut buf, bad) }.is_ok() {
        return Err("copy from unmapped address succeeded");
    }
    if user_copy_active() {
        return Err("user_copy_nest not reset after abort");
    }
    // Repeat (exercises the nested-fault entry path twice) and the store
    // direction; kernel must still be fully alive afterwards.
    if unsafe { copy_to_user(bad as *mut u8, &buf) }.is_ok() {
        return Err("copy to unmapped address succeeded");
    }
    if user_copy_active() {
        return Err("user_copy_nest not reset after store abort");
    }
    Ok(())
}

pub fn test_virt_constants() -> Result<(), &'static str> {
    crate::memory::virt::test_page_constants()
}

pub fn test_shell_dispatch() -> Result<(), &'static str> {
    use alloc::string::String;
    use alloc::vec::Vec;
    let mut lines: Vec<String> = Vec::new();
    {
        let mut sink = |s: &str| lines.push(String::from(s));
        if !crate::shell::commands::dispatch("help", &[], &mut sink, false) {
            return Err("help was not handled");
        }
        if crate::shell::commands::dispatch("theme", &["vahi"], &mut sink, true) {
            return Err("vga-only command ran from GUI dispatch");
        }
    }
    if lines.is_empty() {
        return Err("help produced no output");
    }
    if !lines.concat().contains("exec") {
        return Err("help output missing commands");
    }
    if crate::shell::commands::dispatch("bogus_cmd_xyz", &[], &mut |_| {}, false) {
        return Err("unknown command reported as handled");
    }
    Ok(())
}

#[cfg(feature = "ash")]
pub fn test_ash_hook() -> Result<(), &'static str> {
    use crate::ash::{AshResult, HookPoint, Protocol};
    // Single eBPF insn: r0 += 2 → R0=2 → AshResult::Drop via map_return.
    // Bytecode is exactly 12 bytes (size_of::<EbpfInsn> = 12 with #[repr(C)]).
    // The verifier loads 1 insn; the VM executes it and returns R0=2,
    // which map_return maps to AshResult::Drop.
    let prog = &[
        0x07, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let hook = HookPoint::NetReceive { interface: 0, port: 9999, protocol: Protocol::Udp };
    let id = crate::ash::manager::register(999, prog, hook.clone(), 16, None)
        .map_err(|_| "ash register failed")?;
    let mut packet = [0x42u8; 32];
    let result = crate::ash::hooks::net::hook_net_receive(&mut packet, 0, 17, 1234, 9999);
    crate::ash::manager::unregister(id, 999).map_err(|_| "ash unregister failed")?;
    if result != AshResult::Drop {
        return Err("ash net hook did not drop as programmed");
    }
    Ok(())
}

pub fn register_all() {
    selftest::register("entropy::robust_harvester", test_entropy);
    selftest::register("vfs::page_cache_basic", test_page_cache);
    selftest::register("gdt::selectors_nonzero", test_gdt_selectors);
    selftest::register("gdt::tss_stack", test_tss_stack);
    selftest::register("interrupts::ticks_monotonic", test_ticks);
    selftest::register("phys::bitmap_alloc_free", test_phys_alloc);
    selftest::register("virt::page_constants", test_virt_constants);
    selftest::register("pata::mbr_signature", pata_read_test::test_pata_mbr_sig);
    selftest::register("user_copy::fault_abort_recovers", test_user_copy_fault_abort);
    selftest::register("shell::dispatch_table", test_shell_dispatch);
    #[cfg(feature = "ash")]
    selftest::register("ash::net_hook_fires", test_ash_hook);
}
