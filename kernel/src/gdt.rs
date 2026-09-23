use alloc::boxed::Box;
use core::sync::atomic::Ordering;
use lazy_static::lazy_static;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: crate::sync::IrqSafeMutex<TaskStateSegment> =
        crate::sync::IrqSafeMutex::new(TaskStateSegment::new());
}

pub fn init_tss() {
    let mut tss = TSS.lock();
    // K-03 (D-23): the DF handler runs on this IST stack while the interrupted
    // stack frame is still live. In debug builds the interrupted frame alone
    // can exceed 20 KiB (K-00: CreateAddressSpace overflowed 8 KiB with the
    // handler pushing onto it), so the IST must hold BOTH frames.
    // Evidence: tests/k00_smp1.log df_rsp inside the guard below the old
    // 5-page stack; release 135/135 on 4 pages. 12 pages covers a 36 MB-ELF
    // debug frame (measured ~29 KiB, boot/init.rs) with ~3× headroom — a
    // bounded fix tied to measured evidence, not a blind size increase.
    let df_stack = crate::memory::stack::alloc_stack(12).expect("Failed to allocate DF stack");
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = VirtAddr::new(df_stack.top);

    // Setup Privilege stack (Ring 3 -> 0) with guard page
    let p_stack = crate::memory::stack::alloc_stack(5).expect("Failed to allocate Privilege stack");
    tss.privilege_stack_table[0] = VirtAddr::new(p_stack.top);
}

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
        // We will add TSS entry later after initialization
        (gdt, Selectors {
            code_selector,
            data_selector,
            user_code_selector,
            user_data_selector,
            tss_selector: SegmentSelector(0)
        })
    };
}

#[derive(Debug, Clone, Copy)]
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

static mut SELECTORS: Option<Selectors> = None;

pub fn get_selectors() -> &'static Selectors {
    unsafe {
        (*core::ptr::addr_of!(SELECTORS))
            .as_ref()
            .expect("GDT not initialized")
    }
}

/// Boot-time boot/p_stack top, used only as the initial per-CPU kernel_rsp
/// seed (syscall entry) before the first context switch. After scheduling
/// starts, `set_kernel_stack`/`set_privilege_stack` track the live value.
pub fn get_kernel_stack() -> VirtAddr {
    TSS.lock().privilege_stack_table[0]
}

const MAX_CPUS: usize = 8;

// Per-CPU pointer to the TSS actually loaded into that CPU's GDT (the leaked
// clone). Set by init()/init_ap(). Indexed by LAPIC id, matching the
// scheduler's per-CPU queues.
static mut LOADED_TSS: [*mut TaskStateSegment; MAX_CPUS] = [core::ptr::null_mut(); MAX_CPUS];

fn current_cpu_idx() -> usize {
    core::cmp::min(crate::smp::get_cpu_id(), MAX_CPUS - 1)
}

/// Point ring-3 entry rsp0 at the current thread's own kernel stack top.
/// This is what makes user-mode interrupts (timer, page faults) land on the
/// running thread's kernel stack instead of a single shared privilege stack,
/// so a parked thread's saved context can't be clobbered by the next interrupt.
pub fn set_privilege_stack(top: u64) {
    let tss = unsafe { core::ptr::addr_of_mut!(LOADED_TSS[current_cpu_idx()]).read() };
    if !tss.is_null() {
        unsafe { (*tss).privilege_stack_table[0] = VirtAddr::new(top) };
    }
}

pub fn init() {
    use x86_64::instructions::segmentation::{Segment, CS, DS, SS};
    use x86_64::instructions::tables::load_tss;

    // 1. Initialize TSS stacks with guard pages
    init_tss();

    // 2. Setup GDT with the initialized TSS
    // Leak a reference to the global TSS for the BSP GDT entry
    let tss_ptr = Box::leak(Box::new(TSS.lock().clone()));
    unsafe { LOADED_TSS[0] = tss_ptr as *mut TaskStateSegment };

    let mut gdt = GDT.0.clone();
    let tss_selector = gdt.add_entry(Descriptor::tss_segment(tss_ptr));

    let mut selectors = GDT.1.clone();
    selectors.tss_selector = tss_selector;
    unsafe { *core::ptr::addr_of_mut!(SELECTORS) = Some(selectors) };

    let gdt_static = Box::leak(Box::new(gdt));
    gdt_static.load();

    unsafe {
        // Store user CS/SS for fork_child_return assembly to read
        // Written once before APs start, read-only afterwards (Relaxed ordering ok)
        crate::task::thread::FORK_CHILD_CS
            .store(selectors.user_code_selector.0 as u64 | 3, Ordering::Relaxed);
        crate::task::thread::FORK_CHILD_SS
            .store(selectors.user_data_selector.0 as u64 | 3, Ordering::Relaxed);

        CS::set_reg(selectors.code_selector);
        load_tss(tss_selector);
        DS::set_reg(selectors.data_selector);
        SS::set_reg(selectors.data_selector);
    }
}

/// K-02: CPU identity is established (via `lapic::init` + CPUID fallback)
/// before this runs on an AP, so the CPU id is passed in rather than read
/// through per-CPU machinery (`current_cpu_idx`) that is not valid yet.
pub fn init_ap(cpu_id: usize) {
    use alloc::boxed::Box;
    use x86_64::instructions::segmentation::{Segment, CS, DS, SS};
    use x86_64::instructions::tables::load_tss;

    // Create a per-CPU TSS
    let mut tss = Box::new(TaskStateSegment::new());

    // K-03 (D-23): see init_tss — DF IST must hold the interrupted debug
    // frame plus the handler frame; same measured bound as on the BSP.
    let df_stack = crate::memory::stack::alloc_stack(12).expect("Failed to allocate AP DF stack");
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = VirtAddr::new(df_stack.top);

    let p_stack =
        crate::memory::stack::alloc_stack(5).expect("Failed to allocate AP Privilege stack");
    tss.privilege_stack_table[0] = VirtAddr::new(p_stack.top);

    let tss_ref = Box::leak(tss);
    {
        // Same index the caller derived from its LAPIC id; kept local so
        // this function has no per-CPU/GS dependency.
        let idx = cpu_id;
        unsafe { LOADED_TSS[idx] = tss_ref as *mut TaskStateSegment };
    }

    // Create a per-CPU GDT
    let mut gdt = GlobalDescriptorTable::new();
    let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
    let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
    let _user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
    let _user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
    let tss_selector = gdt.add_entry(Descriptor::tss_segment(tss_ref));

    // Load GDT and segments
    unsafe {
        // We use Box::leak to ensure GDT stays valid.
        // In a real OS we'd track this in a PerCpu structure.
        let gdt_ref = Box::leak(Box::new(gdt));
        gdt_ref.load();

        CS::set_reg(code_selector);
        load_tss(tss_selector);
        DS::set_reg(data_selector);
        SS::set_reg(data_selector);
    }
}
