use core::arch::global_asm;
use crate::println;

use core::sync::atomic::{AtomicU32, Ordering};
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{PageTable, PhysFrame, FrameAllocator};
use x86_64::VirtAddr;

// The trampoline must be placed at a 4KB aligned physical address in the first 1MB.
// We use 0x8000.
pub const TRAMPOLINE_PHYS: u64 = 0x8000;
pub const DATA_PHYS: u64 = 0x7000;

/// Shared data between BSP and APs during boot
#[repr(C)]
pub struct SmpBootData {
    pub stack_ptr: u64,
    pub code_ptr: u64,
    pub cr3: u64,
    pub ap_count: AtomicU32,
}

pub static mut BOOT_DATA: SmpBootData = SmpBootData {
    stack_ptr: 0,
    code_ptr: 0,
    cr3: 0,
    ap_count: AtomicU32::new(0),
};

/// Allocate a copy of the kernel page directory in the lower 4GB.
/// Returns the physical frame address, or None if impossible.
fn allocate_low_pml4() -> Option<PhysFrame> {
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();

    // Get current PML4
    let (current_frame, _) = Cr3::read();
    let current_frame_phys = current_frame.start_address().as_u64();

    // If already below 4GB, use it directly
    if current_frame_phys < 0x1_0000_0000 {
        return Some(current_frame);
    }

    // Otherwise allocate a new frame and copy the kernel entries (indices 256..512)
    let mut fa = crate::memory::buddy::BuddyFrameAllocator;
    let new_frame = fa.allocate_frame()?;
    let new_phys = new_frame.start_address().as_u64();

    if new_phys >= 0x1_0000_0000 {
        // Still above 4GB — cannot use for AP boot
        return None;
    }

    let new_pml4_virt = VirtAddr::new(offset + new_phys);
    let current_pml4_virt = VirtAddr::new(offset + current_frame_phys);

    unsafe {
        let new_pml4 = &mut *(new_pml4_virt.as_mut_ptr() as *mut PageTable);
        let current_pml4 = &*(current_pml4_virt.as_ptr() as *const PageTable);
        // Copy kernel higher-half entries
        for i in 256..512 {
            new_pml4[i] = current_pml4[i].clone();
        }
    }

    Some(new_frame)
}

global_asm!(r#"
.code16
.global smp_trampoline_start
.global smp_trampoline_end

.equ T_GDT_PTR_OFF,   t_gdt_ptr   - smp_trampoline_start
.equ T_PROT_OFF,      t_prot      - smp_trampoline_start
.equ T_GDT64_PTR_OFF, t_gdt64_ptr - smp_trampoline_start
.equ T_LONG_OFF,      t_long      - smp_trampoline_start
.equ T_GDT_OFF,       t_gdt       - smp_trampoline_start
.equ T_GDT64_OFF,     t_gdt64     - smp_trampoline_start

smp_trampoline_start:
    cli
    xor ax, ax
    mov ds, ax
    
    # Load 16-bit GDT
    lgdt [0x8000 + T_GDT_PTR_OFF]
    
    # Switch to protected mode
    mov eax, cr0
    or al, 1
    mov cr0, eax
    
    # Far jump to 32-bit code
    .byte 0x66, 0xEA
    .long 0x8000 + T_PROT_OFF
    .short 0x08

.code32
t_prot:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov esp, 0x9000   # Set stack to identity-mapped area
    
    # Enable PAE + PGE + NXE (CR4 bits 5, 7, 11)
    mov eax, cr4
    or eax, (1 << 5) | (1 << 7) | (1 << 11)
    mov cr4, eax

    # Load page table base from shared data at 0x7000
    mov eax, [0x7000]
    mov cr3, eax

    # Enable long mode (LME) + NXE in EFER
    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 8) | (1 << 11)
    wrmsr
    
    # Enable Paging
    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax
    
    # Load 64-bit GDT
    lgdt [0x8000 + T_GDT64_PTR_OFF]
    
    # Far jump to 64-bit code
    push 0x08
    lea eax, [0x8000 + T_LONG_OFF]
    push eax
    retf

.code64
t_long:
    mov rax, [0x7008] # AP Entry Point
    mov rsp, [0x7010] # AP Stack
    jmp rax

.align 16
t_gdt_ptr:
    .short t_gdt_end - t_gdt - 1
    .long 0x8000 + T_GDT_OFF
t_gdt:
    .quad 0
    .quad 0x00CF9A000000FFFF # 32-bit Code
    .quad 0x00CF92000000FFFF # 32-bit Data
t_gdt_end:

t_gdt64_ptr:
    .short t_gdt64_end - t_gdt64 - 1
    .long 0x8000 + T_GDT64_OFF
t_gdt64:
    .quad 0
    .quad 0x00209A0000000000 # 64-bit Code
    .quad 0x0000920000000000 # 64-bit Data
t_gdt64_end:
smp_trampoline_end:
"#);

extern "C" {
    fn smp_trampoline_start();
    fn smp_trampoline_end();
}

pub fn init() {
    let ap_ids: &alloc::vec::Vec<u8> = match crate::acpi::AP_LAPIC_IDS.get() {
        Some(ids) => ids,
        None => { return; },
    };

    if ap_ids.is_empty() {
        return;
    }

    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();

    // Identity-map trampoline region (0x7000-0x9000) so AP can access it in long mode.
    {
        use x86_64::structures::paging::mapper::TranslateResult;
        use x86_64::structures::paging::*;
        use x86_64::{VirtAddr, PhysAddr};
        let phys_mem_offset = VirtAddr::new(offset);
        let level_4_table = unsafe { crate::memory::active_level_4_table(phys_mem_offset) };
        let mut mapper = unsafe { OffsetPageTable::new(level_4_table, phys_mem_offset) };
        let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        for pa in [0x7000u64, 0x8000u64] {
            let page = Page::<Size4KiB>::containing_address(VirtAddr::new(pa));
            let tr = mapper.translate(page.start_address());
            if !matches!(tr, TranslateResult::Mapped{..}) {
                let frame = PhysFrame::containing_address(PhysAddr::new(pa));
                unsafe {
                    if let Ok(flush) = mapper.map_to(page, frame, flags, &mut frame_allocator) {
                        flush.flush();
                    }
                }
            }
        }
    }

    // 1. Copy Trampoline to 0x8000
    let trampoline_src = unsafe {
        core::slice::from_raw_parts(
            smp_trampoline_start as *const u8,
            smp_trampoline_end as *const () as usize - smp_trampoline_start as *const () as usize
        )
    };
    let trampoline_dest = unsafe {
        core::slice::from_raw_parts_mut((offset + TRAMPOLINE_PHYS) as *mut u8, trampoline_src.len())
    };
    trampoline_dest.copy_from_slice(trampoline_src);

    // 2. Setup Shared Data at 0x7000
    let ap_cr3 = match allocate_low_pml4() {
        Some(frame) => frame.start_address().as_u64(),
        None => {
            println!("SMP: WARNING: Cannot get low-memory CR3, AP boot may fail");
            let (level_4_table_frame, _) = Cr3::read();
            level_4_table_frame.start_address().as_u64()
        }
    };

    unsafe {
        let data_ptr = (offset + DATA_PHYS) as *mut u64;
        *data_ptr.add(0) = ap_cr3; // CR3 (low 32 bits loaded by AP in 32-bit mode)
        *data_ptr.add(1) = ap_kernel_entry as *const () as u64; // Entry Point
    }

    for &ap_id in ap_ids {
        let stack = crate::memory::stack::alloc_stack(8)
            .expect("Failed to allocate AP stack");
        
        unsafe {
             let data_ptr = (offset + DATA_PHYS) as *mut u64;
             *data_ptr.add(2) = stack.top;
        }

        let mut booted = false;
        for attempt in 0..3 {
            if let Some(ref mut lapic) = *crate::apic::lapic::LOCAL_APIC.lock() {
                if attempt == 0 {
                    // INIT IPI
                    lapic.send_ipi(ap_id, 0, 0x05);
                    lapic.wait_for_ipi();
                    // Wait 10ms (spinloop approximation)
                    for _ in 0..10_000_000 { core::hint::spin_loop(); }
                }

                // STARTUP IPI (send twice as per Intel spec)
                let vector = (TRAMPOLINE_PHYS >> 12) as u8;
                for _ in 0..2 {
                    lapic.send_ipi(ap_id, vector, 0x06);
                    lapic.wait_for_ipi();
                    // Short delay between SIPIs (~200us)
                    for _ in 0..200_000 { core::hint::spin_loop(); }
                }
            }

            // Wait with timeout
            let mut timeout = 0u64;
            while unsafe { (*core::ptr::addr_of!(BOOT_DATA)).ap_count.load(Ordering::SeqCst) } == 0 && timeout < 50_000_000 {
                timeout += 1;
                core::hint::spin_loop();
            }

            if unsafe { (*core::ptr::addr_of!(BOOT_DATA)).ap_count.load(Ordering::SeqCst) } > 0 {
                booted = true;
                break;
            }
            println!("SMP: Retry {}/3 for CPU ID {}", attempt + 1, ap_id);
        }

        if booted {
            println!("SMP: CPU ID {} booted successfully", ap_id);
            unsafe { (*core::ptr::addr_of!(BOOT_DATA)).ap_count.store(0, Ordering::SeqCst); }
        } else {
            println!("SMP: WARNING: CPU ID {} failed to boot after 3 attempts", ap_id);
        }
    }

    println!("SMP: Multicore Initialization complete.");
}


#[no_mangle]
pub extern "C" fn ap_kernel_entry() -> ! {
    unsafe {
        (*core::ptr::addr_of!(BOOT_DATA)).ap_count.fetch_add(1, Ordering::SeqCst);
    }
    
    // Each AP needs its own GS base for per-CPU storage (syscalls)
    let cpu_id = { 
        crate::apic::lapic::LOCAL_APIC.lock().as_ref().map(|l| l.id()).unwrap_or(0) as usize
    };
    {
        crate::syscalls::init_gs_base(cpu_id);
    }

    // Each AP needs its own GDT and IDT
    crate::gdt::init_ap();
    crate::interrupts::init_ap();
    
    // Initialize Local APIC for this core
    crate::apic::lapic::init();
    
    // Enable interrupts
    x86_64::instructions::interrupts::enable();
    
    // This core is now ready to be scheduled.
    crate::task::scheduler::schedule();
}
/// Returns the LAPIC ID of the current CPU.
pub fn get_cpu_id() -> usize {
    crate::apic::lapic::LOCAL_APIC.lock().as_ref().map(|l| l.id()).unwrap_or(0) as usize
}

/// Calls a function on a specific CPU core via IPI.
/// `func` must be a pointer to an `extern "C" fn(u64)`.
#[allow(dead_code)]
pub fn smp_call_function(cpu_id: u8, func: extern "C" fn(u64), arg: u64) {
    // Set the function pointer and argument in the target's per-CPU data
    let areas = crate::syscalls::PER_CPU_AREAS.lock();
    if let Some(ptr) = areas.get(cpu_id as usize) {
        let raw = ptr.0;
        if !raw.is_null() {
            unsafe {
                (*raw).ipi_pending = func as u64;
                (*raw).ipi_arg = arg;
            }
        }
    }
    drop(areas);

    if let Some(ref mut lapic) = *crate::apic::lapic::LOCAL_APIC.lock() {
        lapic.send_ipi(cpu_id, 251, 0); // IpiFunc vector
        lapic.wait_for_ipi();
    }
}

/// Broadcasts a function call to all CPU cores except self.
#[allow(dead_code)]
pub fn smp_broadcast(func: extern "C" fn(u64), arg: u64) {
    let areas = crate::syscalls::PER_CPU_AREAS.lock();
    for (_cpu_id, ptr) in areas.iter().enumerate() {
        let raw = ptr.0;
        if !raw.is_null() {
            unsafe {
                (*raw).ipi_pending = func as u64;
                (*raw).ipi_arg = arg;
            }
        }
    }
    drop(areas);

    if let Some(ref mut lapic) = *crate::apic::lapic::LOCAL_APIC.lock() {
        let current_cpu = get_cpu_id() as u8;
        for cpu_id in 0..crate::syscalls::MAX_CPUS as u8 {
            if cpu_id != current_cpu {
                lapic.send_ipi(cpu_id, 251, 0);
                lapic.wait_for_ipi();
            }
        }
    }
}

/// Broadcasts a TLB flush IPI to all other CPU cores.
/// Phase H1: Ensure memory coherence during page table changes.
pub fn broadcast_tlb_flush(addr: u64) {
    if let Some(ref mut lapic) = *crate::apic::lapic::LOCAL_APIC.lock() {
        // Broadcast vector 250 (TlbFlush) to all excluding self
        lapic.send_broadcast_ipi(250);
        lapic.wait_for_ipi();
    }
    let _ = addr;
}
