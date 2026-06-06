use core::sync::atomic::{AtomicU64, Ordering};
use alloc::sync::Arc;
use crate::task::process::Process;
use crate::memory::stack::{Stack, alloc_stack};

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadStatus {
    Ready,
    Running,
        Blocked,
        Exited,
}

#[repr(C, packed)]
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

impl Thread {
    pub fn new(entry_point: extern "C" fn() -> !) -> Self {
        let stack_pages = 8; // 32 KB
        let stack = alloc_stack(stack_pages).expect("Failed to allocate thread stack");
        
        let stack_top = stack.top;
        
        // 16-byte align the stack pointer
        let mut stack_ptr = stack_top & !0xF;
        
        // Reserve space for TaskContext
        stack_ptr -= core::mem::size_of::<TaskContext>() as u64;
        
        let context = TaskContext {
            r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
            rdi: 0, rsi: 0, rbp: 0, rbx: 0, rdx: 0, rcx: 0, rax: 0,
            rip: entry_point as u64,
            rflags: 0x202, // Interrupts enabled
            rsp: stack_ptr, 
        };
        
        unsafe {
            let ptr = stack_ptr as *mut TaskContext;
            core::ptr::write(ptr, context);
        }

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
        }
    }

    pub fn stack_top(&self) -> u64 {
        self.stack.top
    }

    pub fn clone_thread(&self, child_process: Arc<Process>, parent_regs: *const u64, child_stack: u64) -> Self {
        let stack_pages = 8;
        let new_stack = alloc_stack(stack_pages).expect("Failed to allocate child thread stack");

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

        Thread {
            _id: ThreadId::new(),
            stack: new_stack,
            stack_ptr: new_sp,
            status: ThreadStatus::Ready,
            process: Some(child_process),
            priority: self.priority,
            sleep_until: None,
            futex_wake_addr: None,
            pipe_block_key: None,
        }
    }

    pub fn clone_fork(&self, new_process: Arc<Process>, parent_regs: *const u64) -> Self {
        let stack_pages = 8;
        let new_stack = alloc_stack(stack_pages).expect("Failed to allocate child stack");

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

        Thread {
            _id: ThreadId::new(),
            stack: new_stack,
            stack_ptr: new_sp,
            status: ThreadStatus::Ready,
            process: Some(new_process),
            priority: self.priority,
            sleep_until: None,
            futex_wake_addr: None,
            pipe_block_key: None,
        }
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
        ret
    "#
);

core::arch::global_asm!(
    r#"
    .global fork_child_return
    fork_child_return:
        cli
        xor eax, eax            # RAX = 0 (fork returns 0 in child)
        push 0x1B               # SS  = user data segment (0x18 | RPL 3)
        push rsi                # RSP = user_rsp
        push r8                 # RFLAGS = user_rflags
        push 0x23               # CS  = user code segment (0x20 | RPL 3)
        push rdi                # RIP = user_rip
        iretq
    "#
);

extern "C" {
    fn fork_child_return();
}

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
