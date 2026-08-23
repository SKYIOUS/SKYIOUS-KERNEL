//! Diagnostic utilities for IRQ context.
//!
//! `IrqFmtBuf` lets handlers format diagnostics without heap allocation
//! (allocating in IRQ context can deadlock on the global ALLOCATOR spinlock).
//! `soft_lockup_check` detects when a CPU is stuck in a busy loop.
//!
//! Periodic timer diagnostics (mouse state, thread dumps) are gated behind
//! `#[cfg(debug_assertions)]` — they fire every 500 ticks (~5s) and are
//! only useful during development.

use core::sync::atomic::Ordering;

// ─── IrqFmtBuf ─────────────────────────────────────────────────

/// Stack-buffer fmt writer: lets IRQ handlers format diagnostics without
/// touching the heap allocator (allocating there can deadlock on the
/// global ALLOCATOR spinlock — see scheduler::tick docs).
#[cfg(not(target_arch = "aarch64"))]
pub(crate) struct IrqFmtBuf<'a> {
    pub buf: &'a mut [u8],
    pub len: usize,
}

#[cfg(not(target_arch = "aarch64"))]
impl<'a> core::fmt::Write for IrqFmtBuf<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let room = self.buf.len().saturating_sub(self.len);
        let n = room.min(s.len());
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

// ─── Soft-lockup detector ──────────────────────────────────────

/// Soft-lockup detector (per-CPU, stateless refs only, no alloc):
/// flags when the interrupted RIP has been identical for
/// SOFT_LOCKUP_TICKS consecutive ticks while the current thread is not
/// blocked. `Blocked` current thread == idle (the idle incarnation parks
/// a blocked thread), so idle cannot trip this. IF=0 spins stop the timer
/// entirely and are caught by the external QEMU harness, not here.
const SOFT_LOCKUP_TICKS: u64 = 500;

#[cfg(not(target_arch = "aarch64"))]
static LOCKUP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn soft_lockup_check(rip: u64) {
    let cur_rip = rip;
    static SAME: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    static LAST_RIP: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    let last = LAST_RIP.swap(cur_rip, Ordering::Relaxed);
    if last != cur_rip {
        SAME.store(0, Ordering::Relaxed);
        return;
    }

    use crate::task::thread::ThreadStatus;
    let sched = crate::task::scheduler::this_cpu_sched().try_lock();
    let is_blocked_or_none = match sched.as_ref().and_then(|s| s.current_thread.as_ref()) {
        Some(t) => t.status == ThreadStatus::Blocked,
        None => true,
    };
    drop(sched);

    if is_blocked_or_none {
        SAME.store(0, Ordering::Relaxed);
        return;
    }

    let n = SAME.fetch_add(1, Ordering::Relaxed) + 1;
    if n == SOFT_LOCKUP_TICKS && !LOCKUP.swap(true, Ordering::Relaxed) {
        let mut scratch = [0u8; 128];
        let len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let _ = core::fmt::write(&mut w, format_args!(
                "[LOCKUP] rip=0x{:x} stuck {} ticks\n", cur_rip, n));
            len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..len]).unwrap_or(""));
    }
}

// ─── Debug-only timer diagnostics ──────────────────────────────

/// Print the first timer tick (one-shot, fires once at boot).
#[cfg(debug_assertions)]
pub(crate) fn diag_first_tick(ticks: u64) {
    static TICK_DIAG: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if ticks == 1 && !TICK_DIAG.swap(true, Ordering::Relaxed) {
        crate::serial_write("[TICK] first timer tick!\n");
    }
}

/// Print mouse state every 500 ticks (~5s). Stack-buffered, no alloc.
#[cfg(debug_assertions)]
pub(crate) fn diag_mouse_state(ticks: u64) {
    if ticks % 500 != 0 { return; }
    let irq = crate::drivers::mouse::MOUSE_IRQ_COUNT.load(Ordering::Relaxed);
    let bytes = crate::drivers::mouse::MOUSE_IRQ_BYTES.load(Ordering::Relaxed);
    let cx = crate::drivers::mouse::CURSOR_X.load(Ordering::Relaxed);
    let cy = crate::drivers::mouse::CURSOR_Y.load(Ordering::Relaxed);
    let ud = crate::drivers::serial::tx_dropped();
    let mut scratch = [0u8; 128];
    let len;
    {
        let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
        let _ = core::fmt::write(&mut w, format_args!(
            "[TICK={}] mouse irq={} bytes={} pos=({},{}) uart_dropped={}\n",
            ticks, irq, bytes, cx, cy, ud
        ));
        len = w.len;
    }
    crate::serial_write(core::str::from_utf8(&scratch[..len]).unwrap_or(""));
}

/// Dump all scheduler queues every 500 ticks (~5s). Stack-buffered, no alloc.
#[cfg(debug_assertions)]
pub(crate) fn diag_thread_dump(ticks: u64, irq_rip: u64) {
    if ticks % 500 != 0 { return; }
    use crate::task::scheduler::GLOBAL;
    let mut tscratch = [0u8; 512];
    let mut w = IrqFmtBuf { buf: &mut tscratch, len: 0 };
    let cur_pid = crate::task::scheduler::this_cpu_sched().lock().current_thread
        .as_ref().and_then(|t| t.process.as_ref().map(|p| p.id));
    let _ = core::fmt::write(&mut w, format_args!("[THREADS] cur=pid{:?} irq_rip=0x{:x}",
        cur_pid, irq_rip));
    {
        let sched = crate::task::scheduler::this_cpu_sched().lock();
        if let Some(t) = sched.current_thread.as_ref() {
            let pid = t.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!(" curstat=({:?} {:?})", pid, t.status));
        }
        if let Some(t) = sched.switching_old.as_ref() {
            let pid = t.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!(" switching_old=pid{:?} {:?}", pid, t.status));
        }
        let _ = core::fmt::write(&mut w, format_args!(" heap[{}]:", sched.stride_heap.len()));
        for e in sched.stride_heap.iter() {
            let pid = e.0.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!("pid{:?} pass={} ", pid, e.0.pass));
        }
        let _ = core::fmt::write(&mut w, format_args!(" ready[ consolidated ]"));
    }
    if let Some(q) = GLOBAL.sleep_queue.try_lock() {
        let _ = core::fmt::write(&mut w, format_args!("sleep[{}]:", q.len()));
        for t in q.iter() {
            let pid = t.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!(" (pid={:?} {:?} wake={:?})", pid, t.status, t.futex_wake_addr));
        }
    }
    if let Some(q) = GLOBAL.futex_queue.try_lock() {
        let _ = core::fmt::write(&mut w, format_args!(" futex[{}]:", q.len()));
        for t in q.iter() {
            let pid = t.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!(" (pid={:?} {:?} wake={:?})", pid, t.status, t.futex_wake_addr));
        }
    }
    if let Some(q) = GLOBAL.block_queue.try_lock() {
        let _ = core::fmt::write(&mut w, format_args!(" block[{}]:", q.len()));
        for t in q.iter() {
            let pid = t.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!(" (pid={:?} {:?} pipe={:?})", pid, t.status, t.pipe_block_key));
        }
    }
    if let Some(q) = GLOBAL.pending_queue.try_lock() {
        let _ = core::fmt::write(&mut w, format_args!(" pending[{}]", q.len()));
    }
    let _ = core::fmt::write(&mut w, format_args!("\n"));
    let tlen = w.len;
    crate::serial_write(core::str::from_utf8(&tscratch[..tlen]).unwrap_or(""));
}

// No-op stubs for release builds — the compiler eliminates the calls entirely.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub(crate) fn diag_first_tick(_ticks: u64) {}
#[cfg(not(debug_assertions))]
#[inline(always)]
pub(crate) fn diag_mouse_state(_ticks: u64) {}
#[cfg(not(debug_assertions))]
#[inline(always)]
pub(crate) fn diag_thread_dump(_ticks: u64, _irq_rip: u64) {}
