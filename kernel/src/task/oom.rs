//! OOM (Out-of-Memory) killer for the Vahi kernel.
//!
//! When memory allocation fails, the OOM killer selects and terminates
//! the process with the highest OOM score to reclaim memory. It retries
//! allocation after each kill, capping at MAX_KILLS_PER_EVENT to prevent
//! cascading kills.
//!
//! Design inspired by Linux's OOM killer but simplified for Vahi's
//! single-level process model.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::sync::IrqSafeMutex as Mutex;

// ── Constants ────────────────────────────────────────────────────────

/// Maximum processes killed per OOM event to prevent cascading kills.
const MAX_KILLS_PER_EVENT: usize = 16;

/// OOM score adjustment range (matches Linux).
pub const OOM_SCORE_ADJ_MIN: i32 = -1000;
pub const OOM_SCORE_ADJ_MAX: i32 = 1000;

/// Processes with oom_score_adj == -1000 are OOM-protected (init, kernel).
pub const OOM_SCORE_ADJ_PROTECT: i32 = -1000;

/// Score range: 0–1000 for RSS-based component.
const SCORE_RSS_MAX: i32 = 1000;

/// Total OOM score = rss_score + oom_score_adj, clamped to [0, 2000].
const TOTAL_SCORE_MAX: i32 = 2000;

// ── Global state ─────────────────────────────────────────────────────

/// Total kills performed by OOM killer (for /proc/meminfo equivalent).
pub static OOM_KILL_COUNT: AtomicUsize = AtomicUsize::new(0);

lazy_static::lazy_static! {
    /// Per-process OOM score adjustment (set via prctl).
    pub static ref OOM_ADJUSTMENTS: Mutex<hashbrown::HashMap<u64, i32>> =
        Mutex::new(hashbrown::HashMap::new());
}

// ── OOM score computation ───────────────────────────────────────────

/// Compute OOM score from a pre-fetched Process reference.
/// Does NOT lock PROCESS_TABLE — caller must already hold it or
/// have an Arc<Process> that outlives the call.
///
/// Score = RSS component (0–1000) + age bonus + root penalty + oom_score_adj.
/// Returns (total_score, rss_component, adj).
pub fn compute_oom_score_from_proc(
    pid: u64,
    proc: &crate::task::process::Process,
    rss_bytes: u64,
    total_memory_bytes: u64,
) -> (i32, i32, i32) {
    let adj = get_oom_score_adj(pid);

    let rss_score = if total_memory_bytes > 0 {
        ((rss_bytes as i64 * SCORE_RSS_MAX as i64) / total_memory_bytes as i64)
            .min(SCORE_RSS_MAX as i64) as i32
    } else {
        0
    };

    let age_ticks = crate::interrupts::get_ticks().saturating_sub(proc.boot_ticks);
    let age_bonus = (50i32).saturating_sub((age_ticks / 10) as i32).max(0);
    let root_penalty = if proc.creds.lock().uid == 0 { -100 } else { 0 };

    let total = (rss_score + age_bonus + root_penalty + adj).clamp(0, TOTAL_SCORE_MAX);
    (total, rss_score, adj)
}

/// Compute OOM score — locks PROCESS_TABLE internally.
/// Use compute_oom_score_from_proc when you already hold the lock.
pub fn compute_oom_score(
    pid: u64,
    rss_bytes: u64,
    total_memory_bytes: u64,
) -> (i32, i32, i32) {
    let table = crate::task::process::PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        compute_oom_score_from_proc(pid, proc, rss_bytes, total_memory_bytes)
    } else {
        (0, 0, 0)
    }
}

/// Get the OOM score adjustment for a process.
pub fn get_oom_score_adj(pid: u64) -> i32 {
    OOM_ADJUSTMENTS
        .lock()
        .get(&pid)
        .copied()
        .unwrap_or(0)
}

/// Set the OOM score adjustment for a process.
pub fn set_oom_score_adj(pid: u64, adj: i32) -> Result<(), crate::syscalls::errno::Errno> {
    let adj = adj.clamp(OOM_SCORE_ADJ_MIN, OOM_SCORE_ADJ_MAX);
    OOM_ADJUSTMENTS.lock().insert(pid, adj);
    Ok(())
}

// ── Process memory estimation ────────────────────────────────────────

/// Estimate a process's resident memory in bytes.
///
/// Counts mapped user pages from the process's VMA list.
/// This is an approximation — exact RSS would require page table walks.
pub fn estimate_process_rss(proc: &crate::task::process::Process) -> u64 {
    let vmas = proc.memory.lock().vmas.clone();
    let mut rss_bytes: u64 = 0;

    for vma in vmas.iter() {
        // Skip kernel mappings
        if vma.start >= 0xFFFF_8000_0000_0000 {
            continue;
        }
        rss_bytes += vma.end.saturating_sub(vma.start);
    }

    rss_bytes
}

/// Get total system memory in bytes.
pub fn total_system_memory() -> u64 {
    let free = crate::memory::phys::total_free_frames() as u64;
    let heap_size = crate::allocator::HEAP_SIZE as u64;
    free * 4096 + heap_size
}

// ── Victim selection ─────────────────────────────────────────────────

/// Information about a candidate OOM victim.
pub struct OomVictim {
    pub pid: u64,
    pub score: i32,
    pub rss_bytes: u64,
    pub name: alloc::string::String,
}

/// Select the best OOM victim from all processes.
///
/// Returns the process with the highest OOM score. Protected processes
/// (oom_score_adj == -1000) and the current process are excluded.
pub fn select_victim() -> Option<OomVictim> {
    let current_pid = crate::task::process::CURRENT_PROCESS
        .lock()
        .as_ref()
        .map(|p| p.id);

    let table = crate::task::process::PROCESS_TABLE.lock();
    let total_mem = total_system_memory();

    let mut best: Option<OomVictim> = None;

    for (&pid, proc) in table.iter() {
        // Skip current process (it's the one asking for memory)
        if Some(pid) == current_pid {
            continue;
        }

        let adj = get_oom_score_adj(pid);

        // Skip OOM-protected processes
        if adj == OOM_SCORE_ADJ_PROTECT {
            continue;
        }

        let rss = estimate_process_rss(proc);
        let (score, _, _) = compute_oom_score_from_proc(pid, proc, rss, total_mem);

        let name = proc.name.lock().clone();

        match &best {
            None => {
                best = Some(OomVictim {
                    pid,
                    score,
                    rss_bytes: rss,
                    name,
                });
            }
            Some(current_best) => {
                if score > current_best.score
                    || (score == current_best.score && rss > current_best.rss_bytes)
                {
                    best = Some(OomVictim {
                        pid,
                        score,
                        rss_bytes: rss,
                        name,
                    });
                }
            }
        }
    }

    best
}

// ── OOM event ────────────────────────────────────────────────────────

/// Result of an OOM event.
pub struct OomEvent {
    pub kills: usize,
    pub freed_bytes: u64,
    pub allocation_succeeded: bool,
}

/// Execute the OOM killer: repeatedly kill the highest-scored process
/// and retry allocation until it succeeds or we hit the kill limit.
///
/// Uses only stack-allocated buffers — safe to call under memory pressure
/// (no heap allocation via format! — that would recurse into handle_oom).
pub fn oom_kill_event() -> OomEvent {
    let mut kills = 0usize;
    let mut freed_total: u64 = 0;

    crate::serial_write("[OOM] Memory pressure detected, starting OOM killer\n");

    for _ in 0..MAX_KILLS_PER_EVENT {
        let victim = match select_victim() {
            Some(v) => v,
            None => {
                crate::serial_write("[OOM] No more killable processes\n");
                break;
            }
        };

        // Stack-formatted kill message (no heap allocation).
        {
            let mut buf = [0u8; 160];
            let mut pos = copy_bytes(&mut buf, 0, b"[OOM] Killing pid=");
            pos = write_u64(&mut buf, pos, victim.pid);
            pos = copy_bytes(&mut buf, pos, b" score=");
            pos = write_i32(&mut buf, pos, victim.score);
            pos = copy_bytes(&mut buf, pos, b" rss=");
            pos = write_u64(&mut buf, pos, victim.rss_bytes / 1024);
            pos = copy_bytes(&mut buf, pos, b"KB\n");
            crate::serial_write(core::str::from_utf8(&buf[..pos]).unwrap_or(""));
        }

        let rss_freed = victim.rss_bytes;

        kill_victim(victim.pid);

        kills += 1;
        freed_total += rss_freed;
        OOM_KILL_COUNT.fetch_add(1, Ordering::Relaxed);

        reclaim_memory();

        let free_pages = crate::memory::phys::total_free_frames();
        if free_pages >= 256 {
            // Stack-formatted reclaim message.
            let mut buf = [0u8; 128];
            let mut pos = copy_bytes(&mut buf, 0, b"[OOM] Reclaimed ");
            pos = write_u64(&mut buf, pos, freed_total / 1024);
            pos = copy_bytes(&mut buf, pos, b"KB from ");
            pos = write_u64(&mut buf, pos, kills as u64);
            pos = copy_bytes(&mut buf, pos, b" kills, ");
            pos = write_u64(&mut buf, pos, free_pages as u64);
            pos = copy_bytes(&mut buf, pos, b" pages free\n");
            crate::serial_write(core::str::from_utf8(&buf[..pos]).unwrap_or(""));
            return OomEvent {
                kills,
                freed_bytes: freed_total,
                allocation_succeeded: true,
            };
        }
    }

    crate::serial_write("[OOM] Exhausted kills, system may be unstable.\n");

    OomEvent {
        kills,
        freed_bytes: freed_total,
        allocation_succeeded: false,
    }
}

/// Copy a byte slice into buf at offset, return new offset.
fn copy_bytes(buf: &mut [u8], pos: usize, src: &[u8]) -> usize {
    let n = src.len().min(buf.len() - pos);
    buf[pos..pos + n].copy_from_slice(&src[..n]);
    pos + n
}

/// Write i32 as decimal into buf at offset.
fn write_i32(buf: &mut [u8], pos: usize, val: i32) -> usize {
    if val < 0 {
        let pos = copy_bytes(buf, pos, b"-");
        return write_u64(buf, pos, (-(val as i64)) as u64);
    }
    write_u64(buf, pos, val as u64)
}

// ── Kill a specific victim ──────────────────────────────────────────

/// Kill a process by PID, handling cleanup properly.
fn kill_victim(pid: u64) {
    let proc = {
        let mut table = crate::task::process::PROCESS_TABLE.lock();
        table.remove(&pid)
    };

    let Some(proc) = proc else {
        return;
    };

    // Send SIGKILL (signal 9) to all threads of the process
    proc.signals
        .lock()
        .raise(crate::syscalls::signal::Signal::_SIGKILL);

    // Free swap mappings
    {
        let mut swap_map = proc.memory.lock().swap_map.clone();
        for (_, &(dev_idx, slot_idx)) in swap_map.iter() {
            crate::memory::swap::free_swap_slot(dev_idx, slot_idx);
        }
        swap_map.clear();
    }

    // Free all mapped user pages
    {
        let vmas = proc.memory.lock().vmas.clone();
        for vma in vmas.iter() {
            if vma.start >= 0xFFFF_8000_0000_0000 {
                continue;
            }
            let page_count = (vma.end - vma.start) / 4096;
            for i in 0..page_count {
                let virt_addr = vma.start + i * 4096;
                let virt = x86_64::VirtAddr::new(virt_addr);
                let page =
                    x86_64::structures::paging::Page::<
                        x86_64::structures::paging::Size4KiB,
                    >::containing_address(virt);

                let mapper = unsafe { proc.address_space.mapper() };
                if let Some(mut mapper) = mapper {
                    use x86_64::structures::paging::{Mapper, Translate};
                    if let x86_64::structures::paging::mapper::TranslateResult::Mapped {
                        frame,
                        ..
                    } = mapper.translate(virt)
                    {
                        if crate::memory::frame_info::count(frame.start_address()) == 1 {
                            if let Ok((oframe, flusher)) = mapper.unmap(page) {
                                flusher.flush();
                                crate::memory::frame_info::decrement(oframe.start_address());
                                crate::memory::phys::free_frame(oframe.start_address().as_u64());
                            }
                        }
                    }
                }
            }
        }
    }

    // Notify parent with SIGCHLD
    if let Some(parent_id) = proc.parent_id {
        let table = crate::task::process::PROCESS_TABLE.lock();
        if let Some(parent) = table.get(&parent_id) {
            parent
                .signals
                .lock()
                .raise(crate::syscalls::signal::Signal::SIGCHLD);
            crate::task::process::route_signal_to_signalfd(
                parent.id, 17, crate::task::process::SI_CHILD, pid, 0, 0,
            );
        }
    }

    // Close all file descriptors
    {
        let mut fd_table = proc.files.lock().fd_table.clone();
        for fd_opt in fd_table.iter_mut() {
            fd_opt.take();
        }
    }

    // Stack-formatted cleanup message (no heap allocation).
    {
        let mut buf = [0u8; 48];
        let mut pos = copy_bytes(&mut buf, 0, b"[OOM] Cleaned up pid=");
        pos = write_u64(&mut buf, pos, pid);
        pos = copy_bytes(&mut buf, pos, b"\n");
        crate::serial_write(core::str::from_utf8(&buf[..pos]).unwrap_or(""));
    }
}

// ── Memory reclaim ──────────────────────────────────────────────────

/// Attempt to reclaim memory after killing a process.
fn reclaim_memory() {
    for _ in 0..16 {
        if !crate::memory::swap::try_evict_one_page() {
            break;
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// The main OOM handler called from `handle_alloc_error`.
///
/// This replaces the old `crate::oom_kill()` which just killed the
/// current process blindly. The new implementation:
///
/// 1. Identifies the highest-scoring victim (not current process)
/// 2. Kills it and reclaims memory
/// 3. Retries up to MAX_KILLS_PER_EVENT times
/// 4. Falls back to halting if no memory can be freed
pub fn handle_oom() -> ! {
    crate::serial_write("[OOM] handle_oom: entering OOM killer\n");

    let event = oom_kill_event();

    if event.allocation_succeeded {
        crate::serial_write("[OOM] Memory reclaimed successfully\n");
        crate::task::scheduler::schedule();
    } else {
        crate::serial_write("[OOM] FATAL: Cannot reclaim enough memory\n");
    }

    crate::serial_write("[OOM] System halted. Manual reboot required.\n");
    loop {
        x86_64::instructions::hlt();
    }
}

/// Get OOM statistics for /proc/meminfo.
pub fn oom_stats() -> (usize, u64) {
    let kills = OOM_KILL_COUNT.load(Ordering::Relaxed);
    let adj_count = OOM_ADJUSTMENTS.lock().len();
    (kills, adj_count as u64)
}

// ── Proactive memory pressure detection ────────────────────────────

/// Percentage of total memory below which we consider pressure.
/// 5% = 20 pages per 4096 total, scales with system size.
const PRESSURE_THRESHOLD_PERCENT: u64 = 5;

/// Minimum free pages to consider healthy (floor for tiny systems).
const MIN_FREE_PAGES: u64 = 256;

/// Check if the system is under memory pressure and trigger OOM if so.
/// Called from the timer tick handler every ~100 ticks (~1 second).
///
/// SAFETY: Uses try_lock on PROCESS_TABLE to avoid deadlocking when
/// called from IRQ context while the interrupted code holds the lock.
/// If the lock is contended, the check is skipped — the allocation
/// failure path (handle_alloc_error) provides a second chance.
pub fn check_memory_pressure() {
    let free_pages = crate::memory::phys::total_free_frames() as u64;
    let total_pages = total_system_memory() / 4096;

    let threshold = core::cmp::max(
        MIN_FREE_PAGES,
        (total_pages * PRESSURE_THRESHOLD_PERCENT) / 100,
    );

    if free_pages < threshold {
        // Use stack buffer to avoid heap allocation in IRQ context.
        let mut buf = [0u8; 128];
        let msg = write_pressure_msg(&mut buf, free_pages, threshold);
        crate::serial_write(msg);
        // Only run OOM killer if PROCESS_TABLE is not contended.
        // In IRQ context, spinning on another CPU's lock is a livelock
        // (IF=0, timer can't fire on this CPU). The allocation failure
        // path (handle_alloc_error) provides a fallback.
        if crate::task::process::PROCESS_TABLE.try_lock().is_some() {
            oom_kill_event();
        }
    }
}

/// Format the pressure message into a stack buffer (no heap allocation).
fn write_pressure_msg<'a>(buf: &'a mut [u8], free: u64, threshold: u64) -> &'a str {
    // Manual integer-to-decimal formatting to avoid alloc::format!.
    let mut pos = 0;
    let prefix = b"[OOM] Memory pressure: ";
    buf[..prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    pos = write_u64(buf, pos, free);
    let mid = b" pages free, ";
    buf[pos..pos + mid.len()].copy_from_slice(mid);
    pos += mid.len();
    pos = write_u64(buf, pos, threshold);
    let suffix = b" threshold\n";
    if pos + suffix.len() <= buf.len() {
        buf[pos..pos + suffix.len()].copy_from_slice(suffix);
        pos += suffix.len();
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("[OOM] pressure\n")
}

/// Write a u64 as decimal into buf at offset, return new offset.
fn write_u64(buf: &mut [u8], mut pos: usize, val: u64) -> usize {
    if val == 0 {
        if pos < buf.len() { buf[pos] = b'0'; pos += 1; }
        return pos;
    }
    // Write digits in reverse, then flip.
    let start = pos;
    let mut v = val;
    while v > 0 && pos < buf.len() {
        buf[pos] = b'0' + (v % 10) as u8;
        v /= 10;
        pos += 1;
    }
    // Reverse the digits.
    let digits = pos - start;
    let mut i = start;
    let mut j = start + digits - 1;
    while i < j {
        buf.swap(i, j);
        i += 1;
        j -= 1;
    }
    pos
}
