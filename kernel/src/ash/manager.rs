use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;
use crate::ash::{
    AshHandler, HookPoint, VerifiedAsh, AshResult, AshError, AshStats,
};

lazy_static! {
    static ref ASH_MANAGER: Mutex<AshManagerInner> = Mutex::new(AshManagerInner::new());
}

struct AshManagerInner {
    handlers: BTreeMap<u64, AshHandler>,
    verified: BTreeMap<u64, VerifiedAsh>,
    next_id: u64,
    total_insns: u64,
    total_events: u64,
    total_dropped: u64,
    total_handled: u64,
    total_modified: u64,
    total_errors: u64,
}

impl AshManagerInner {
    fn new() -> Self {
        AshManagerInner {
            handlers: BTreeMap::new(),
            verified: BTreeMap::new(),
            next_id: 1,
            total_insns: 0,
            total_events: 0,
            total_dropped: 0,
            total_handled: 0,
            total_modified: 0,
            total_errors: 0,
        }
    }
}

/// Register a new ASH handler for the given process.
pub fn register(pid: u64, bytecode: &[u8], hook: HookPoint, max_insns: u32, expiry: Option<u64>) -> Result<u64, AshResult> {
    let mut mgr = ASH_MANAGER.lock();

    let id = mgr.next_id;
    mgr.next_id = mgr.next_id.wrapping_add(1);

    // ponytail: simple linear dedup — compares bytecode+hoook identity
    let is_dup = mgr.handlers.values().any(|h| {
        h.pid == pid && h.bytecode.as_slice() == bytecode && h.hook_point == hook
    });
    if is_dup {
        return Err(AshResult::Error(AshError::Unknown));
    }

    let handler = AshHandler {
        id,
        pid,
        bytecode: bytecode.to_vec(),
        hook_point: hook,
        context_mask: 0,
        max_insns,
        expiry,
    };

    let verified = crate::ash::verifier::verify_handler(&handler)?;

    mgr.handlers.insert(id, handler);
    mgr.verified.insert(id, verified);
    Ok(id)
}

/// Unregister a handler by ID. Only the owning process can unregister.
pub fn unregister(handler_id: u64, pid: u64) -> Result<(), AshResult> {
    let mut mgr = ASH_MANAGER.lock();
    let handler = mgr.handlers.get(&handler_id).ok_or(AshResult::Error(AshError::NotFound))?;
    if handler.pid != pid {
        return Err(AshResult::Error(AshError::Unknown));
    }
    mgr.handlers.remove(&handler_id);
    mgr.verified.remove(&handler_id);
    Ok(())
}

/// Unregister all handlers belonging to a process (called on process exit).
pub fn unregister_all(pid: u64) {
    let mut mgr = ASH_MANAGER.lock();
    let ids: Vec<u64> = mgr.handlers.iter()
        .filter(|(_, h)| h.pid == pid)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        mgr.handlers.remove(&id);
        mgr.verified.remove(&id);
    }
}

/// Look up verified handlers matching a hook point.
/// Returns handler IDs to avoid lifetime issues with the mutex.
pub fn lookup_ids(hook: &HookPoint) -> Vec<u64> {
    let mgr = ASH_MANAGER.lock();
    mgr.handlers.iter()
        .filter(|(_, h)| hook_matches(&h.hook_point, hook))
        .map(|(id, _)| *id)
        .collect()
}

/// Get verified handler by ID (caller must hold no ASH_MANAGER lock).
pub fn get_verified(id: u64) -> Option<VerifiedAsh> {
    let mgr = ASH_MANAGER.lock();
    mgr.verified.get(&id).cloned()
}

/// Check if a handler's hook point matches a triggered hook.
fn hook_matches(registered: &HookPoint, triggered: &HookPoint) -> bool {
    match (registered, triggered) {
        (HookPoint::NetReceive { protocol: rp, port: rport, .. },
         HookPoint::NetReceive { protocol: tp, port: tport, .. }) => {
            (rp == tp || *rport == 0) && (*rport == 0 || *rport == *tport)
        }
        (HookPoint::SyscallEntry { syscall_num: r }, HookPoint::SyscallEntry { syscall_num: t }) => r == t,
        (HookPoint::SyscallExit { syscall_num: r }, HookPoint::SyscallExit { syscall_num: t }) => r == t,
        (HookPoint::TimerFired { timer_id: r }, HookPoint::TimerFired { timer_id: t }) => r == t,
        (HookPoint::SignalDelivery { signal: r }, HookPoint::SignalDelivery { signal: t }) => r == t,
        (HookPoint::MessageReceive { channel: r }, HookPoint::MessageReceive { channel: t }) => r == t,
        _ => false,
    }
}

/// Record execution stats after running a handler.
pub fn record_execution(result: &AshResult, cycles: u64) {
    let mut mgr = ASH_MANAGER.lock();
    mgr.total_events = mgr.total_events.wrapping_add(1);
    mgr.total_insns = mgr.total_insns.wrapping_add(cycles);
    match result {
        AshResult::Drop => mgr.total_dropped = mgr.total_dropped.wrapping_add(1),
        AshResult::Handled => mgr.total_handled = mgr.total_handled.wrapping_add(1),
        AshResult::Modified => mgr.total_modified = mgr.total_modified.wrapping_add(1),
        AshResult::Error(_) => mgr.total_errors = mgr.total_errors.wrapping_add(1),
        _ => {}
    }
}

/// Get ASH statistics for a process.
pub fn process_stats(_pid: u64) -> AshStats {
    let mgr = ASH_MANAGER.lock();
    AshStats {
        total_insns: mgr.total_insns,
        total_events: mgr.total_events,
        total_dropped: mgr.total_dropped,
        total_handled: mgr.total_handled,
        total_modified: mgr.total_modified,
        total_errors: mgr.total_errors,
    }
}

/// Initialize the ASH manager.
pub fn init() {
    lazy_static::initialize(&ASH_MANAGER);
}
