//! Boot state machine runner.
//!
//! Each state is a function that takes `&BootContext` and returns
//! `Result<BootState, BootError>`. The main loop validates transitions
//! and logs every state change.

use crate::boot::{
    BootState, BootError, BootEvent, BootWarning, BootContext, BootSession, logger::BootLogger,
};

/// Run the boot state machine to completion.
/// This function never returns — it either enters userspace or panics.
pub fn run_boot() -> ! {
    use crate::interrupts::get_ticks;
    let mut ctx = BootContext::new(get_ticks());
    let mut session = BootSession {
        entry_point: 0,
        user_rsp: 0,
    };

    let mut state = BootState::InitKernel;
    loop {
        ctx.trace.push(BootEvent::Enter(state));
        BootLogger::info(&ctx, &alloc::format!("{:?}", state));

        let next = match state {
            BootState::InitKernel => state_init_kernel(&ctx),
            BootState::LocateInit => state_locate_init(&mut ctx),
            BootState::ParseElf => state_parse_elf(&mut ctx, &mut session),
            BootState::CreateAddressSpace => state_create_address_space(&mut ctx, &mut session),
            BootState::MapStack => state_map_stack(&mut ctx, &mut session),
            BootState::CreatePid1 => state_create_pid1(&mut ctx, &mut session),
            BootState::SetupConsole => state_setup_console(&mut ctx, &mut session),
            BootState::EnterUserspace => state_enter_userspace(&ctx, &session),
            BootState::Running => {
                BootLogger::info(&ctx, "Boot complete, entering scheduler dispatch");
                // Running is terminal — hand off to scheduler. schedule()
                // returns only when this thread is the sole runnable work;
                // then idle-wait (the timer IRQ's try_schedule does all
                // future switching).
                crate::task::scheduler::schedule();
                loop {
                    x86_64::instructions::interrupts::enable_and_hlt();
                }
            }
            BootState::Failed => {
                // Panic handler will dump trace
                panic!("Boot failed — see trace above");
            }
        };

        match next {
            Ok(next_state) => {
                // Validate transition
                let valid = state.valid_next();
                if !valid.contains(&next_state) && next_state != BootState::Failed {
                    BootLogger::error(&ctx, &alloc::format!(
                        "Invalid boot transition: {:?} → {:?}", state, next_state
                    ));
                    ctx.trace.push(BootEvent::Error(BootError::UserspaceEntryFailed));
                    panic!("Invalid boot state transition");
                }
                ctx.trace.push(BootEvent::Exit(state));
                state = next_state;
            }
            Err(e) => {
                ctx.trace.push(BootEvent::Error(e));
                BootLogger::error(&ctx, &alloc::format!("{:?}", e));
                let err_str = alloc::format!("Boot failed at {:?}: {:?}", state, e);
                ctx.trace.push(BootEvent::Exit(state));
                // Dump trace then panic
                BootLogger::error(&ctx, "Boot trace:");
                for event in &ctx.trace {
                    BootLogger::error(&ctx, &alloc::format!("  {:?}", event));
                }
                panic!("{}", err_str);
            }
        }
    }
}

use alloc::sync::Arc;
use crate::sync::IrqSafeMutex as Mutex;
use crate::task::process::Process;

static BOOT_PROCESS: Mutex<Option<Arc<Process>>> = Mutex::new(None);

fn state_init_kernel(_ctx: &BootContext) -> Result<BootState, BootError> {
    Ok(BootState::LocateInit)
}

fn state_locate_init(ctx: &mut BootContext) -> Result<BootState, BootError> {
    let search_paths = ["/bin/init", "/init", "/sbin/init"];
    let vfs_mgr = crate::vfs::VFS.lock();
    for path in &search_paths {
        ctx.init_paths_tried.push(alloc::string::String::from(*path));
        BootLogger::info(ctx, &alloc::format!("Looking for {}", path));
        if let Some(node) = vfs_mgr.resolve_path(path) {
            if let Ok(data) = node.read(usize::MAX) {
                ctx.elf_data = Some(data);
                drop(vfs_mgr);
                BootLogger::info(ctx, &alloc::format!("Found init at {}", path));
                return Ok(BootState::ParseElf);
            }
        }
    }
    drop(vfs_mgr);
    Err(BootError::InitNotFound)
}

fn state_parse_elf(ctx: &mut BootContext, _session: &mut BootSession) -> Result<BootState, BootError> {
    let elf_data = ctx.elf_data.as_ref().ok_or(BootError::InitNotFound)?;
    if elf_data.len() < 64 || &elf_data[..4] != b"\x7fELF" {
        return Err(BootError::InvalidElf);
    }
    BootLogger::info(ctx, "ELF header valid");
    Ok(BootState::CreateAddressSpace)
}

fn state_create_address_space(ctx: &mut BootContext, session: &mut BootSession) -> Result<BootState, BootError> {
    use crate::memory::buddy::BuddyFrameAllocator;
    let mut frame_allocator = BuddyFrameAllocator;
    let elf_data = ctx.elf_data.as_ref().ok_or(BootError::InitNotFound)?;
    let address_space = crate::memory::paging::AddressSpace::new(&mut frame_allocator)
        .ok_or(BootError::AddressSpaceCreationFailed)?;
    let process = Process::load_elf(elf_data, address_space)
        .map_err(|_| BootError::InvalidElf)?;
    session.entry_point = process.entry_point;
    *BOOT_PROCESS.lock() = Some(Arc::new(process));
    BootLogger::info(ctx, &alloc::format!("PID 1 ELF loaded, entry=0x{:x}", session.entry_point));
    Ok(BootState::MapStack)
}

fn state_map_stack(ctx: &mut BootContext, session: &mut BootSession) -> Result<BootState, BootError> {
    let process_guard = BOOT_PROCESS.lock();
    let process = process_guard.as_ref().ok_or(BootError::StackAllocationFailed)?;
    // Activate the process address space BEFORE mapping the user stack, so
    // virt_to_phys (which walks the active tables) can translate the stack
    // pages setup_user_stack maps into the process's own page tables.
    // SAFETY: the address space was fully set up by Process::load_elf() in
    // CreateAddressSpace and shares the kernel higher-half mapping.
    unsafe { process.address_space.activate(); }
    let argv = alloc::vec![alloc::string::String::from("/bin/init")];
    let user_rsp = process.setup_user_stack(&argv)
        .map_err(|_| BootError::StackAllocationFailed)?;
    session.user_rsp = user_rsp;
    BootLogger::info(ctx, &alloc::format!("User stack at 0x{:x}", user_rsp));
    Ok(BootState::CreatePid1)
}

fn state_create_pid1(ctx: &mut BootContext, _session: &BootSession) -> Result<BootState, BootError> {
    let mut process_guard = BOOT_PROCESS.lock();
    let process = process_guard.take().ok_or(BootError::StackAllocationFailed)?;
    let pid = process.id;
    Process::register(process.clone());
    *process_guard = Some(process);
    drop(process_guard);
    BootLogger::info(ctx, &alloc::format!("PID 1 registered (pid={})", pid));
    Ok(BootState::SetupConsole)
}

fn state_setup_console(ctx: &mut BootContext, _session: &BootSession) -> Result<BootState, BootError> {
    let process_guard = BOOT_PROCESS.lock();
    let process = process_guard.as_ref().ok_or(BootError::ConsoleUnavailable)?;
    let tty_node = crate::vfs::VFS.lock().resolve_path("/dev/tty0");
    match tty_node {
        Some(tty) => {
            use crate::task::process::FileDescriptor;
            let mut fd_table = process.files.lock().fd_table.clone();
            fd_table.resize(3, None);
            fd_table[0] = Some(FileDescriptor::File { node: tty.clone(), offset: crate::sync::IrqSafeMutex::new(0) });
            fd_table[1] = Some(FileDescriptor::File { node: tty.clone(), offset: crate::sync::IrqSafeMutex::new(0) });
            fd_table[2] = Some(FileDescriptor::File { node: tty, offset: crate::sync::IrqSafeMutex::new(0) });
            drop(fd_table);
            // Keep fd_flags in lockstep with fd_table — exec/fork clone both,
            // and a short flags vector makes dup2/fcntl index out of bounds.
            // Access-mode bits (0=O_RDONLY, 1=O_WRONLY): fd0 read, fd1/fd2 write.
            process.files.lock().fd_flags = alloc::vec![0u64, 1, 1];
            BootLogger::info(ctx, "stdin/stdout/stderr -> /dev/tty0");
        }
        None => {
            ctx.trace.push(BootEvent::Warning(BootWarning::ConsoleUnavailable));
            BootLogger::warn(ctx, "/dev/tty0 not found — init runs with no stdin/stdout/stderr");
        }
    }
    crate::task::scheduler::with_current_thread(|thread| {
        thread.process = Some(process.clone());
    });
    BootLogger::info(ctx, "Thread process assigned");
    Ok(BootState::EnterUserspace)
}

fn state_enter_userspace(ctx: &BootContext, session: &BootSession) -> Result<BootState, BootError> {
    crate::boot::store_trace(ctx.trace.clone(), ctx.init_paths_tried.clone());
    let process_guard = BOOT_PROCESS.lock();
    let process = process_guard.as_ref().ok_or(BootError::UserspaceEntryFailed)?;
    *crate::task::process::CURRENT_PROCESS.lock() = Some(process.clone());
    BootLogger::info(ctx, "Activating address space");
    // SAFETY: The address space was fully set up by Process::load_elf() in state CreateAddressSpace
    // and contains the init binary with proper page tables. No other CPU is using it.
    unsafe { process.address_space.activate(); }
    BootLogger::info(ctx, &alloc::format!("Jumping to userspace entry=0x{:x} rsp=0x{:x}", session.entry_point, session.user_rsp));
    // SAFETY: All prerequisite setup is complete — valid ELF loaded into the address space,
    // user stack mapped with argv, PID 1 registered in the process table, and the address
    // space activated. This function diverges (never returns).
    unsafe {
        crate::task::thread::jump_to_usermode(session.entry_point, session.user_rsp);
    }
}
