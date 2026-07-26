//! Boot state machine runner.
//!
//! Each state is a function that takes `&BootContext` and returns
//! `Result<BootState, BootError>`. The main loop validates transitions
//! and logs every state change.

use crate::boot::{
    BootState, BootError, BootEvent, BootContext, BootSession, logger::BootLogger,
};

/// Run the boot state machine to completion.
/// This function never returns — it either enters userspace or panics.
pub fn run_boot() -> ! {
    use crate::interrupts::get_ticks;
    let mut ctx = BootContext::new(get_ticks());
    let mut session = BootSession {
        elf_data: None,
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
                // Running is terminal — hand off to scheduler
                crate::task::scheduler::schedule();
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

// ponytail: Stub implementations — replaced by Task 3
fn state_init_kernel(_ctx: &BootContext) -> Result<BootState, BootError> {
    todo!()
}
fn state_locate_init(_ctx: &mut BootContext) -> Result<BootState, BootError> {
    todo!()
}
fn state_parse_elf(_ctx: &mut BootContext, _session: &mut BootSession) -> Result<BootState, BootError> {
    todo!()
}
fn state_create_address_space(_ctx: &mut BootContext, _session: &mut BootSession) -> Result<BootState, BootError> {
    todo!()
}
fn state_map_stack(_ctx: &mut BootContext, _session: &mut BootSession) -> Result<BootState, BootError> {
    todo!()
}
fn state_create_pid1(_ctx: &mut BootContext, _session: &mut BootSession) -> Result<BootState, BootError> {
    todo!()
}
fn state_setup_console(_ctx: &mut BootContext, _session: &mut BootSession) -> Result<BootState, BootError> {
    todo!()
}
fn state_enter_userspace(_ctx: &BootContext, _session: &BootSession) -> Result<BootState, BootError> {
    todo!()
}
