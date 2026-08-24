use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use alloc::boxed::Box;
use alloc::sync::Arc;
use crate::task::process::Process;
use crate::memory::stack::{Stack, alloc_stack};

// ─── FPU/SSE/XSAVE state (x86_64 only) ─────────────────────────

#[cfg(target_arch = "x86_64")]
const FPU_AREA_SIZE: usize = 4096;

#[cfg(target_arch = "x86_64")]
#[repr(C, align(64))]
pub struct FpuArea {
    pub data: [u8; FPU_AREA_SIZE],
}

#[cfg(target_arch = "x86_64")]
impl FpuArea {
    pub fn new() -> Self {
        FpuArea { data: [0u8; FPU_AREA_SIZE] }
    }
}

#[cfg(target_arch = "x86_64")]
pub static HAS_XSAVE: AtomicBool = AtomicBool::new(false);

#[cfg(target_arch = "x86_64")]
pub unsafe fn save_fpu(buf: &mut FpuArea) {
    if HAS_XSAVE.load(Ordering::Relaxed) {
        let rfbm: u64 = !0;
        core::arch::asm!(
            "xsave64 [{0}]",
            in(reg) buf.data.as_mut_ptr(),
            in("edx") (rfbm >> 32) as u32,
            in("eax") rfbm as u32,
            options(nostack, preserves_flags)
        );
    } else {
        core::arch::asm!(
            "fxsave64 [{0}]",
            in(reg) buf.data.as_mut_ptr(),
            options(nostack, preserves_flags)
        );
    }
}

#[cfg(target_arch = "x86_64")]
pub unsafe fn restore_fpu(buf: &FpuArea) {
    if HAS_XSAVE.load(Ordering::Relaxed) {
        let rfbm: u64 = !0;
        core::arch::asm!(
            "xrstor64 [{0}]",
            in(reg) buf.data.as_ptr(),
            in("edx") (rfbm >> 32) as u32,
            in("eax") rfbm as u32,
            options(nostack, preserves_flags)
        );
    } else {
        core::arch::asm!(
            "fxrstor64 [{0}]",
            in(reg) buf.data.as_ptr(),
            options(nostack, preserves_flags)
        );
    }
}

#[cfg(target_arch = "x86_64")]
pub fn detect_xsave() {
    unsafe {
        let mut eax: u32;
        let mut ecx: u32;
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            lateout("ecx") ecx,
            lateout("eax") eax,
            out("edx") _,
            options(nostack, preserves_flags)
        );
        if (ecx >> 27) & 1 != 0 {
            HAS_XSAVE.store(true, Ordering::Relaxed);
        }
    }
}

/// User-mode CS/SS selectors (with RPL 3). Initialized by gdt::init().
/// Read from assembly in fork_child_return — AtomicU64 stores a plain u64 at offset 0
/// so the asm `mov r9, qword ptr [rip + ...]` reads the value correctly.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub static FORK_CHILD_CS: AtomicU64 = AtomicU64::new(0x23);
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub static FORK_CHILD_SS: AtomicU64 = AtomicU64::new(0x1B);

pub static HAS_FSGSBASE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(u64);

impl ThreadId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        ThreadId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct Thread {
    pub _id: ThreadId,
    pub stack: Stack,
    pub stack_ptr: u64,
    pub status: ThreadStatus,
    pub process: Option<Arc<Process>>,
    pub priority: u8,
    pub sleep_until: Option<u64>,
    pub futex_wake_addr: Option<u64>,
    pub pipe_block_key: Option<u64>,
    pub fs_base: u64,
    /// One-shot: this thread's first scheduling is a fork/clone child, whose
    /// `ret` goes straight to user space (`fork_child_return` iretq) and never
    /// returns through the post-switch `route_switching_old`. The scheduler
    /// must therefore route the parent it switches away from directly.
    pub first_switch_pending: bool,
    // ── Stride scheduling fields ──────────────────────────────────
    /// Accumulated virtual-pass value. The thread with the smallest pass
    /// among all ready threads runs next.
    pub pass: u64,
    /// stride = STRIDE_MAX / tickets (set on priority change / init).
    pub stride: u64,
    /// Proportional-share tickets (higher = more CPU). Default 20.
    pub tickets: u32,
    // ── RT scheduling fields ──────────────────────────────────────
    /// Scheduling policy: 0=SCHED_OTHER (CFS/stride), 1=SCHED_FIFO, 2=SCHED_RR
    pub policy: u32,
    /// RT priority (1-99 for FIFO/RR, ignored for OTHER)
    pub rt_priority: u32,
    /// Time quantum remaining for SCHED_RR (ticks)
    pub rr_time_slice: u32,
    /// Scheduling class: 0=SCHED_NORMAL, 1=SCHED_FIFO, 2=SCHED_RR, 3=SCHED_BATCH
    pub sched_class: u32,
    // ── CPU affinity ──────────────────────────────────────────────
    /// CPU affinity mask (bit i = can run on CPU i)
    pub affinity_mask: u64,
    // ── FPU state (x86_64 only) ────────────────────────────────
    #[cfg(target_arch = "x86_64")]
    pub fpu_state: Option<Box<FpuArea>>,
}

/// Maximum stride value (virtual time units). Must be >> max threads.
pub const STRIDE_MAX: u64 = 1 << 20;

/// Default tickets for a normal thread.
pub const DEFAULT_TICKETS: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadStatus {
    Ready,
    Running,
        Blocked,
        Exited,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TaskContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub rflags: u64,
    pub rip: u64,
    pub rsp: u64,
}

/// aarch64 context frame: 16 u64s (128 bytes).
/// Matches the layout expected by `arch_aarch64::switch_thread`.
#[cfg(target_arch = "aarch64")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AArch64ContextFrame {
    pub x19: u64, pub x20: u64,
    pub x21: u64, pub x22: u64,
    pub x23: u64, pub x24: u64,
    pub x25: u64, pub x26: u64,
    pub x27: u64, pub x28: u64,
    pub x29: u64, pub x30: u64,
    pub sp_el0: u64,
    pub tpidr_el0: u64,
    pub old_fpu_ptr: u64,
    pub new_fpu_ptr: u64,
}

impl Thread {
    pub fn new(entry_point: extern "C" fn() -> !) -> Self {
        let stack_pages = 8; // 32 KB
        let stack = alloc_stack(stack_pages).expect("Failed to allocate thread stack");
        
        let stack_top = stack.top;
        
        // 16-byte align the stack pointer (required by both x86_64 and aarch64)
        let mut stack_ptr = stack_top & !0xF;

        #[cfg(target_arch = "x86_64")]
        {
            // Reserve space for TaskContext (x86_64)
            stack_ptr -= core::mem::size_of::<TaskContext>() as u64;
            let context = TaskContext {
                r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
                rdi: 0, rsi: 0, rbp: 0, rbx: 0, rdx: 0, rcx: 0, rax: 0,
                rip: entry_point as u64,
                rflags: 0x202, // Interrupts enabled
                rsp: stack_ptr,
            };
            unsafe {
                core::ptr::write(stack_ptr as *mut TaskContext, context);
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            // Reserve space for AArch64ContextFrame (16 u64s = 128 bytes)
            stack_ptr -= core::mem::size_of::<AArch64ContextFrame>() as u64;
            // The frame is zero-initialized (all regs 0, FPU pointers 0 = null).
            // x30 (LR) = entry_point so `ret` jumps there.
            let frame = AArch64ContextFrame {
                x30: entry_point as u64,
                ..AArch64ContextFrame {
                    x19: 0, x20: 0, x21: 0, x22: 0,
                    x23: 0, x24: 0, x25: 0, x26: 0,
                    x27: 0, x28: 0, x29: 0,
                    sp_el0: 0, tpidr_el0: 0,
                    old_fpu_ptr: 0, new_fpu_ptr: 0,
                }
            };
            unsafe {
                core::ptr::write(stack_ptr as *mut AArch64ContextFrame, frame);
            }
        }

        let stride = if DEFAULT_TICKETS > 0 { STRIDE_MAX / DEFAULT_TICKETS as u64 } else { STRIDE_MAX };

        Thread {
            _id: ThreadId::new(),
            stack,
            stack_ptr, 
            status: ThreadStatus::Ready,
            process: None,
            priority: 3, 
            sleep_until: None,
            futex_wake_addr: None,
            pipe_block_key: None,
            fs_base: 0,
            pass: 0,
            stride,
            tickets: DEFAULT_TICKETS,
            first_switch_pending: false,
            policy: 0, // SCHED_OTHER
            rt_priority: 0,
            rr_time_slice: 0,
            sched_class: 0, // SCHED_NORMAL
            affinity_mask: 0xFFFFFFFFFFFFFFFF, // All CPUs

            #[cfg(target_arch = "x86_64")]
            fpu_state: None,
        }
    }

    /// Recalculate stride when tickets change.
    #[allow(dead_code)]
    pub fn set_tickets(&mut self, tickets: u32) {
        self.tickets = tickets;
        self.stride = if tickets > 0 { STRIDE_MAX / tickets as u64 } else { STRIDE_MAX };
    }

    pub fn stack_top(&self) -> u64 {
        self.stack.top
    }

    #[cfg(target_arch = "x86_64")]
    pub fn clone_thread(&self, child_process: Arc<Process>, parent_regs: *const u64, child_stack: u64) -> Option<Self> {
        let stack_pages = 8;
        let new_stack = alloc_stack(stack_pages)?;

        let stack_top = new_stack.top;
        let mut new_sp = stack_top & !0xF;
        new_sp -= core::mem::size_of::<TaskContext>() as u64;

        let user_r15 = unsafe { *parent_regs.add(0) };
        let user_r14 = unsafe { *parent_regs.add(1) };
        let user_r13 = unsafe { *parent_regs.add(2) };
        let user_r12 = unsafe { *parent_regs.add(3) };
        let user_r11 = unsafe { *parent_regs.add(4) };
        let user_r10 = unsafe { *parent_regs.add(5) };
        let user_r9  = unsafe { *parent_regs.add(6) };
        let user_rbp = unsafe { *parent_regs.add(10) };
        let user_rbx = unsafe { *parent_regs.add(11) };
        let user_rdx = unsafe { *parent_regs.add(12) };
        let user_rcx = unsafe { *parent_regs.add(13) };
        let user_rip = unsafe { *parent_regs.add(15) };
        let user_rflags = unsafe { *parent_regs.add(16) };

        let context = TaskContext {
            r15: user_r15,
            r14: user_r14,
            r13: user_r13,
            r12: user_r12,
            r11: user_r11,
            r10: user_r10,
            r9:  user_r9,
            r8:  user_rflags,
            rdi: user_rip,
            rsi: child_stack,
            rbp: user_rbp,
            rbx: user_rbx,
            rdx: user_rdx,
            rcx: user_rcx,
            rax: 0,
            rflags: user_rflags,
            rip: fork_child_return as *const () as u64,
            rsp: child_stack,
        };

        unsafe {
            core::ptr::write(new_sp as *mut TaskContext, context);
        }

        Some(Thread {
            _id: ThreadId::new(),
            stack: new_stack,
            stack_ptr: new_sp,
            status: ThreadStatus::Ready,
            process: Some(child_process),
            priority: self.priority,
            sleep_until: None,
            futex_wake_addr: None,
            pipe_block_key: None,
            fs_base: self.fs_base,
            pass: 0,        // fresh pass for child
            stride: self.stride,
            tickets: self.tickets,
            first_switch_pending: true,
            policy: self.policy,
            rt_priority: self.rt_priority,
            rr_time_slice: 0, // RR child starts with fresh quantum
            sched_class: self.sched_class,
            affinity_mask: self.affinity_mask,

            #[cfg(target_arch = "x86_64")]
            fpu_state: None,
        })
    }

    #[cfg(target_arch = "aarch64")]
    pub fn clone_thread(&self, child_process: Arc<Process>, parent_regs: *const u64, child_stack: u64) -> Option<Self> {
        let stack_pages = 8;
        let new_stack = alloc_stack(stack_pages)?;
        let stack_top = new_stack.top;
        let mut new_sp = stack_top & !0xF;
        new_sp -= core::mem::size_of::<AArch64ContextFrame>() as u64;

        // parent_regs is the aarch64 exception frame (save_all):
        //   [0..30]  = x0..x30
        //   [30]     = x30 (LR)
        //   [31]     = ELR_EL1
        //   [32]     = SPSR_EL1
        //   [33]     = SP_EL0
        // On aarch64, clone_thread sets up child to return to userspace
        // via the exception return path (eret from switch_thread).
        let user_x30 = unsafe { *parent_regs.add(30) };  // LR (return address)
        let user_elr = unsafe { *parent_regs.add(31) };  // ELR_EL1
        let user_spsr = unsafe { *parent_regs.add(32) }; // SPSR_EL1
        let user_sp_el0 = if child_stack != 0 { child_stack } else {
            unsafe { *parent_regs.add(33) }  // SP_EL0
        };

        // x0 = 0 (fork returns 0 in child)
        let frame = AArch64ContextFrame {
            x19: 0, x20: 0, x21: 0, x22: 0,
            x23: 0, x24: 0, x25: 0, x26: 0,
            x27: 0, x28: 0, x29: 0,
            x30: user_x30,
            sp_el0: user_sp_el0,
            tpidr_el0: self.fs_base,
            old_fpu_ptr: 0, new_fpu_ptr: 0,
        };
        // Note: ELR_EL1 and SPSR_EL1 are in the exception frame, not the
        // switch_frame. The child will return via the exception return path
        // (restore_all in the vector table) which reads ELR_EL1/SPSR_EL1
        // from the exception frame. For a simple fork, we set up the child
        // to return to the user entry point via the iret-equivalent path.
        // TODO: proper aarch64 fork return via exception frame
        unsafe {
            core::ptr::write(new_sp as *mut AArch64ContextFrame, frame);
        }

        Some(Thread {
            _id: ThreadId::new(),
            stack: new_stack,
            stack_ptr: new_sp,
            status: ThreadStatus::Ready,
            process: Some(child_process),
            priority: self.priority,
            sleep_until: None,
            futex_wake_addr: None,
            pipe_block_key: None,
            fs_base: self.fs_base,
            pass: 0,
            stride: self.stride,
            tickets: self.tickets,
            first_switch_pending: true,
            policy: self.policy,
            rt_priority: self.rt_priority,
            rr_time_slice: 0,
            sched_class: self.sched_class,
            affinity_mask: self.affinity_mask,
        })
    }

    #[cfg(target_arch = "x86_64")]
    pub fn clone_fork(&self, new_process: Arc<Process>, parent_regs: *const u64) -> Option<Self> {
        let stack_pages = 8;
        let new_stack = alloc_stack(stack_pages)?;

        // Build a switch_context-compatible context near the top of the child's
        // kernel stack (same layout as Thread::new). We do NOT copy the parent's
        // entire stack because the syscall-entry register-format differs from
        // what switch_context expects.
        let stack_top = new_stack.top;
        let mut new_sp = stack_top & !0xF;
        new_sp -= core::mem::size_of::<TaskContext>() as u64;

        // Read user register values from parent's syscall‑entry context.
        // parent_regs points to r15 at offset 0 of the 18‑value save area:
        //   [r15, r14, r13, r12, r11, r10, r9, r8, rdi, rsi, rbp, rbx, rdx,
        //    rcx, rax, rcx(=user_rip), r11(=user_rflags), gs:[0x10](=user_rsp)]
        let user_r15 = unsafe { *parent_regs.add(0) };
        let user_r14 = unsafe { *parent_regs.add(1) };
        let user_r13 = unsafe { *parent_regs.add(2) };
        let user_r12 = unsafe { *parent_regs.add(3) };
        let user_r11 = unsafe { *parent_regs.add(4) };
        let user_r10 = unsafe { *parent_regs.add(5) };
        let user_r9  = unsafe { *parent_regs.add(6) };
        let _user_r8  = unsafe { *parent_regs.add(7) };
        let _user_rdi = unsafe { *parent_regs.add(8) };
        let _user_rsi = unsafe { *parent_regs.add(9) };
        let user_rbp = unsafe { *parent_regs.add(10) };
        let user_rbx = unsafe { *parent_regs.add(11) };
        let user_rdx = unsafe { *parent_regs.add(12) };
        let user_rcx = unsafe { *parent_regs.add(13) };
        let _user_rax = unsafe { *parent_regs.add(14) }; // syscall number (57 for fork)
        let user_rip = unsafe { *parent_regs.add(15) };  // offset 120 = user_rip
        let user_rflags = unsafe { *parent_regs.add(16) }; // offset 128 = user_rflags
        let user_rsp = unsafe { *parent_regs.add(17) };  // offset 136 = user_rsp

        let context = TaskContext {
            r15: user_r15,
            r14: user_r14,
            r13: user_r13,
            r12: user_r12,
            r11: user_r11,
            r10: user_r10,
            r9:  user_r9,
            r8:  user_rflags,   // trampoline: mov r11, r8
            rdi: user_rip,      // trampoline: mov rcx, rdi
            rsi: user_rsp,      // trampoline: mov rsp, rsi
            rbp: user_rbp,
            rbx: user_rbx,
            rdx: user_rdx,
            rcx: user_rcx,
            rax: 0,             // fork returns 0 in the child
            rflags: user_rflags,
            rip: fork_child_return as *const () as u64,
            rsp: user_rsp,
        };

        unsafe {
            core::ptr::write(new_sp as *mut TaskContext, context);
        }

        Some(Thread {
            _id: ThreadId::new(),
            stack: new_stack,
            stack_ptr: new_sp,
            status: ThreadStatus::Ready,
            process: Some(new_process),
            priority: self.priority,
            sleep_until: None,
            futex_wake_addr: None,
            pipe_block_key: None,
            fs_base: self.fs_base,
            pass: 0,        // fresh pass for forked child
            stride: self.stride,
            tickets: self.tickets,
            first_switch_pending: true,
            policy: self.policy,
            rt_priority: self.rt_priority,
            rr_time_slice: 0,
            sched_class: self.sched_class,
            affinity_mask: self.affinity_mask,

            #[cfg(target_arch = "x86_64")]
            fpu_state: None,
        })
    }

    #[cfg(target_arch = "aarch64")]
    pub fn clone_fork(&self, new_process: Arc<Process>, parent_regs: *const u64) -> Option<Self> {
        let stack_pages = 8;
        let new_stack = alloc_stack(stack_pages)?;
        let stack_top = new_stack.top;
        let mut new_sp = stack_top & !0xF;
        new_sp -= core::mem::size_of::<AArch64ContextFrame>() as u64;

        // parent_regs is the aarch64 exception frame (34 u64s):
        //   [0..30]  = x0..x30
        //   [31]     = ELR_EL1
        //   [32]     = SPSR_EL1
        //   [33]     = SP_EL0
        let user_x30 = unsafe { *parent_regs.add(30) };
        let user_sp_el0 = unsafe { *parent_regs.add(33) };

        let frame = AArch64ContextFrame {
            x19: 0, x20: 0, x21: 0, x22: 0,
            x23: 0, x24: 0, x25: 0, x26: 0,
            x27: 0, x28: 0, x29: 0,
            x30: user_x30,
            sp_el0: user_sp_el0,
            tpidr_el0: self.fs_base,
            old_fpu_ptr: 0, new_fpu_ptr: 0,
        };
        unsafe {
            core::ptr::write(new_sp as *mut AArch64ContextFrame, frame);
        }

        Some(Thread {
            _id: ThreadId::new(),
            stack: new_stack,
            stack_ptr: new_sp,
            status: ThreadStatus::Ready,
            process: Some(new_process),
            priority: self.priority,
            sleep_until: None,
            futex_wake_addr: None,
            pipe_block_key: None,
            fs_base: self.fs_base,
            pass: 0,
            stride: self.stride,
            tickets: self.tickets,
            first_switch_pending: true,
            policy: self.policy,
            rt_priority: self.rt_priority,
            rr_time_slice: 0,
            sched_class: self.sched_class,
            affinity_mask: self.affinity_mask,
        })
    }
}

const MSR_FS_BASE: u32 = 0xC0000100;

/// Read the current FS base.
#[allow(dead_code)]
pub fn read_fs_base() -> u64 {
    if HAS_FSGSBASE.load(Ordering::Relaxed) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let base: u64;
            core::arch::asm!("rdfsbase {0}", out(reg) base, options(att_syntax));
            return base;
        }
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        return x86_64::registers::model_specific::Msr::new(MSR_FS_BASE).read();
    }
    #[cfg(not(target_arch = "x86_64"))]
    0
}

/// Write FS base.
pub fn write_fs_base(base: u64) {
    if HAS_FSGSBASE.load(Ordering::Relaxed) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!("wrfsbase {0}", in(reg) base, options(att_syntax));
            return;
        }
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        x86_64::registers::model_specific::Msr::new(MSR_FS_BASE).write(base);
    }
}

/// Switch threads: saves/restores FPU state, FS base, then context switches.
/// Only available on x86_64; aarch64 uses the Arch trait implementation.
#[cfg(target_arch = "x86_64")]
pub fn switch_thread(
    old_rsp: *mut u64,
    new_rsp: u64,
    new_fs_base: u64,
    old_fpu: *mut FpuArea,
    new_fpu: *const FpuArea,
) {
    // Save outgoing thread's FPU state
    if !old_fpu.is_null() {
        unsafe { save_fpu(&mut *old_fpu); }
    }
    // Set CR0.TS so the incoming thread gets #NM on first FPU use.
    // The #NM handler clears CR0.TS, so this is a no-op if FPU was never used.
    unsafe {
        core::arch::asm!("mov rax, cr0; or rax, 0x8; mov cr0, rax", out("rax") _, options(nostack));
    }
    write_fs_base(new_fs_base);
    unsafe { switch_context(old_rsp, new_rsp); }
    // We are now the incoming thread. Clear CR0.TS and restore our FPU state.
    unsafe { core::arch::asm!("clts", options(nostack)); }
    if !new_fpu.is_null() {
        unsafe { restore_fpu(&*new_fpu); }
    }
}

extern "C" {
    pub fn switch_context(old_rsp: *mut u64, new_rsp: u64);
}

core::arch::global_asm!(
    r#"
    .global switch_context
    switch_context:
        # Disable interrupts so the switch is atomic
        cli
        # Save current context
        pushfq
        push rax
        push rcx
        push rdx
        push rbx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15
        
        # Switch stack pointer
        mov [rdi], rsp
        mov rsp, rsi
        
        # Restore next context
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rbx
        pop rdx
        pop rcx
        pop rax
        popfq
        sti
        ret
    "#
);

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    r#"
    .global fork_child_return
    fork_child_return:
        cli
        xor eax, eax            # RAX = 0 (fork returns 0 in child)
        mov r9, qword ptr [rip + FORK_CHILD_SS]
        push r9                 # SS  = user data selector | RPL 3 (dynamic)
        push rsi                # RSP = user_rsp
        push r8                 # RFLAGS = user_rflags
        mov r9, qword ptr [rip + FORK_CHILD_CS]
        push r9                 # CS  = user code selector | RPL 3 (dynamic)
        push rdi                # RIP = user_rip
        xor esi, esi
        xor r8d, r8d
        xor edi, edi
        iretq
    "#
);

#[cfg(target_arch = "x86_64")]
extern "C" {
    fn fork_child_return();
}

/// PHASE D1: jump_to_usermode(entry: u64, user_rsp: u64) -> !

/// PHASE D1: jump_to_usermode(entry: u64, user_rsp: u64) -> !
/// Constructs a synthetic iret frame on kernel stack and jumps to Ring 3.
pub unsafe fn jump_to_usermode(entry: u64, user_rsp: u64) -> ! {
    use crate::gdt;
    let selectors = gdt::get_selectors();
    
    let ss = selectors.user_data_selector.0 | 3;
    let cs = selectors.user_code_selector.0 | 3;
    let rflags = 0x202; // IF=1, IOPL=0
    
    // SAFETY: We are switching to Ring 3. This is inherently unsafe and requires 
    // valid user-mode segments to be present in the GDT.
    // NOTE: Do NOT `mov gs, ax` — that would reset GS base to 0 (user segment).
    // The syscall entry code uses `swapgs` to get the PerCpuData GS base; we must
    // preserve the GS base by skipping `mov gs, ax`. Then swapgs back before iretq
    // so that the next syscall entry can swap it in again.
    core::arch::asm!(
        "mov ds, ax",
        "mov es, ax",
        "mov fs, ax",
        "swapgs",
        "mov rdi, rsi",  // Pass user_rsp to _start so it can read argc/argv from stack
        "push rax",      // SS
        "push rsi",      // RSP
        "push r8",       // RFLAGS
        "push rcx",      // CS
        "push rdx",      // RIP
        "iretq",
        in("rax") ss as u64,
        in("rsi") user_rsp,
        in("r8") rflags,
        in("rcx") cs as u64,
        in("rdx") entry,
        options(noreturn)
    );
}
