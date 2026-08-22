//! Control Flow Integrity (CFI) — Software CFI for indirect calls
//!
//! Validates that function pointers point to known-good targets before
//! allowing indirect calls. Protects against:
//! - Corrupted function pointers (buffer overflows, use-after-free)
//! - ROP/JOP attacks that hijack control flow
//! - Kernel object corruption that overwrites callback pointers
//!
//! Design:
//! - Dynamic table: runtime-registered targets (populated at init + ASH hooks)
//! - Binary search O(log n) for static, linear scan for dynamic
//! - Violation logging with optional panic mode

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

/// Maximum number of static CFI targets
const MAX_STATIC_TARGETS: usize = 1024;

/// Global CFI state
pub struct CfiState {
    /// Static valid targets (sorted for binary search, populated at init)
    static_targets: Mutex<Vec<usize>>,
    /// Dynamic valid targets (runtime-registered, e.g. ASH hooks, RCU callbacks)
    dynamic_targets: Mutex<Vec<usize>>,
    /// Whether CFI enforcement is enabled
    enabled: AtomicBool,
    /// Whether CFI violations cause a panic
    panic_on_violation: AtomicBool,
    /// Whether to log violations to serial
    log_violations: AtomicBool,
    /// Whether initialization is complete
    initialized: AtomicBool,
    /// Violation counter
    violation_count: AtomicUsize,
}

impl CfiState {
    const fn new() -> Self {
        Self {
            static_targets: Mutex::new(Vec::new()),
            dynamic_targets: Mutex::new(Vec::new()),
            enabled: AtomicBool::new(true),
            panic_on_violation: AtomicBool::new(false), // Log-only by default
            log_violations: AtomicBool::new(true),
            initialized: AtomicBool::new(false),
            violation_count: AtomicUsize::new(0),
        }
    }
}

/// Global CFI state
pub static CFI_STATE: CfiState = CfiState::new();

/// Initialize CFI. Called once during kernel initialization after heap is ready.
///
/// Populates the static target table with known valid indirect call targets.
/// Each subsystem can also register additional targets via cfi_register_target.
pub fn cfi_init() {
    let mut targets = CFI_STATE.static_targets.lock();

    // Register core kernel functions that are called via transmuted pointers.
    // SMP entry point — called when APs boot
    register_static(&mut targets, crate::smp::ap_kernel_entry as *const () as usize);

    // RCU synchronize — called via function pointer
    register_static(&mut targets, crate::sync::rcu::synchronize_rcu as *const () as usize);

    // Sort for binary search
    targets.sort_unstable();

    CFI_STATE.initialized.store(true, Ordering::Release);
    crate::serial_write("[CFI] Initialized with ");
    crate::serial_write(&alloc::format!("{} static targets\n", targets.len()));
}

/// Register a static target at boot time (internal helper)
fn register_static(targets: &mut Vec<usize>, addr: usize) {
    if targets.len() < MAX_STATIC_TARGETS && !targets.contains(&addr) {
        targets.push(addr);
    }
}

/// Register a dynamic target at runtime (e.g., ASH hook, RCU callback).
/// Call this when registering a function pointer that will be called later.
pub fn cfi_register_target(addr: usize) {
    if addr == 0 {
        return;
    }
    let mut targets = CFI_STATE.dynamic_targets.lock();
    if !targets.contains(&addr) {
        targets.push(addr);
    }
}

/// Unregister a dynamic target (e.g., when ASH hook is removed).
pub fn cfi_unregister_target(addr: usize) {
    let mut targets = CFI_STATE.dynamic_targets.lock();
    if let Some(pos) = targets.iter().position(|&a| a == addr) {
        targets.swap_remove(pos);
    }
}

/// Check if a function pointer is a valid CFI target.
/// Returns true if the pointer is valid, false if it's a CFI violation.
///
/// This is the core CFI check — called before any indirect call through
/// a potentially-corrupted function pointer.
#[inline]
pub fn cfi_check(addr: usize) -> bool {
    if !CFI_STATE.enabled.load(Ordering::Relaxed) {
        return true;
    }

    if addr == 0 {
        return false; // Null function pointer is always a violation
    }

    // Check static targets (sorted, binary search)
    {
        let targets = CFI_STATE.static_targets.lock();
        if targets.binary_search(&addr).is_ok() {
            return true;
        }
    }

    // Check dynamic targets (unsorted, linear scan — smaller set)
    {
        let targets = CFI_STATE.dynamic_targets.lock();
        if targets.iter().any(|&a| a == addr) {
            return true;
        }
    }

    // CFI violation detected
    cfi_violation(addr);
    false
}

/// Handle a CFI violation
fn cfi_violation(addr: usize) {
    let count = CFI_STATE.violation_count.fetch_add(1, Ordering::Relaxed) + 1;

    if CFI_STATE.log_violations.load(Ordering::Relaxed) {
        crate::serial_write("[CFI] VIOLATION #");
        crate::serial_write(&alloc::format!("{}", count));
        crate::serial_write(": invalid call target 0x");
        crate::serial_write(&alloc::format!("{:x}", addr));
        crate::serial_write("\n");
    }

    if CFI_STATE.panic_on_violation.load(Ordering::Relaxed) {
        panic!(
            "[CFI] Control Flow Integrity violation: invalid call target 0x{:x}",
            addr
        );
    }
}

/// Get the number of CFI violations detected
pub fn cfi_violation_count() -> usize {
    CFI_STATE.violation_count.load(Ordering::Relaxed)
}

/// Enable/disable CFI enforcement at runtime
pub fn cfi_set_enabled(enabled: bool) {
    CFI_STATE.enabled.store(enabled, Ordering::Relaxed);
}

/// Set whether CFI violations cause a panic
pub fn cfi_set_panic_on_violation(panic: bool) {
    CFI_STATE.panic_on_violation.store(panic, Ordering::Relaxed);
}

/// Safe wrapper: validate and call an indirect function pointer.
/// Returns Some(result) if the call was valid, None if CFI violation.
pub unsafe fn cfi_call<F, R>(addr: usize, call: F) -> Option<R>
where
    F: FnOnce() -> R,
{
    if cfi_check(addr) {
        Some(call())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cfi_null_pointer() {
        assert!(!cfi_check(0));
    }

    #[test]
    fn test_cfi_valid_target() {
        let addr = test_function as usize;
        cfi_register_target(addr);
        assert!(cfi_check(addr));
    }

    #[test]
    fn test_cfi_invalid_target() {
        // An arbitrary address should fail
        assert!(!cfi_check(0xDEAD_BEEF));
    }

    #[test]
    fn test_cfi_unregister() {
        let addr = test_function as usize;
        cfi_register_target(addr);
        assert!(cfi_check(addr));
        cfi_unregister_target(addr);
        assert!(!cfi_check(addr));
    }

    #[test]
    fn test_cfi_violation_count() {
        let before = cfi_violation_count();
        cfi_check(0xCAFE_BABE);
        assert!(cfi_violation_count() > before);
    }

    fn test_function() {}
}
