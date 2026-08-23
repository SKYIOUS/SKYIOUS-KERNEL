//! Timer tick processing.
//!
//! Runs in IRQ context with IF=0 and can preempt any syscall mid-critical-section.
//! A blocking spin here is a permanent deadlock (the preempted holder cannot
//! run until we iret), so every lock is taken non-blocking and contended work
//! is deferred to the next tick.

use super::{this_cpu_sched, GLOBAL};

/// Process timer tick: wake sleeping threads, tick POSIX timers, ITIMER_REAL, accumulate CPU time.
pub fn tick(current_ticks: u64) {
    crate::syscalls::posix_timers::check_posix_timers();

    let mut sched = match this_cpu_sched().try_lock() {
        Some(s) => s,
        None => return,
    };

    if let Some(mut sleep) = GLOBAL.sleep_queue.try_lock() {
        // Rotate in place instead of draining into a new VecDeque: queue
        // growth allocates, and we are in IRQ context (IF=0).
        let n = sleep.len();
        let mut woken = 0u32;
        for _ in 0..n {
            let Some(mut thread) = sleep.pop_front() else { break };
            let mut wake = false;
            if let Some(wake_time) = thread.sleep_until {
                if current_ticks >= wake_time { wake = true; }
            }
            if !wake {
                if let Some(ref proc) = thread.process {
                    if let Some(sig) = proc.signals.try_lock() {
                        if sig.has_unmasked_pending(sig.blocked) { wake = true; }
                    }
                }
            }
            if wake {
                thread.status = crate::task::thread::ThreadStatus::Ready;
                thread.sleep_until = None;
                let p = (thread.priority as usize).min(7);
                sched.ready_queues[p].push_back(thread);
                woken += 1;
            } else {
                sleep.push_back(thread);
            }
        }
        if woken > 0 { sched.mark_ready_queues_dirty(); }
    }

    // Accumulate CPU time for current thread's process
    if let Some(ref cur) = sched.current_thread {
        if let Some(ref proc) = cur.process {
            proc.utime.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
    drop(sched);

    // Decrement ITIMER_REAL for every process. Fixed stack array instead of a
    // Vec: IRQ context must not allocate.
    let mut itimer_pids: [u64; 64] = [0; 64];
    let mut itimer_count = 0usize;
    {
        let table = match crate::task::process::PROCESS_TABLE.try_lock() {
            Some(t) => t,
            None => return,
        };
        for pid in table.keys() {
            if itimer_count >= itimer_pids.len() { break; }
            itimer_pids[itimer_count] = *pid;
            itimer_count += 1;
        }
    }
    for &pid in &itimer_pids[..itimer_count] {
        let table = match crate::task::process::PROCESS_TABLE.try_lock() {
            Some(t) => t,
            None => return,
        };
        let Some(proc) = table.get(&pid) else { continue };
        let mut it = match proc.itimer_real.try_lock() {
            Some(it) => it,
            None => continue,
        };
        if it.it_value.tv_sec > 0 || it.it_value.tv_usec > 0 {
            let tick_usec = 10_000u64; // 10ms per tick
            let remaining_usec = (it.it_value.tv_sec as u64) * 1_000_000 + it.it_value.tv_usec as u64;
            if remaining_usec <= tick_usec {
                // Timer expired
                it.it_value = it.it_interval; // reload
                if let Some(mut sig) = proc.signals.try_lock() {
                    sig.raise(crate::syscalls::signal::Signal::_SIGALRM);
                }
                let wakeable = GLOBAL.block_queue.try_lock().is_some()
                    && GLOBAL.futex_queue.try_lock().is_some()
                    && this_cpu_sched().try_lock().is_some();
                if wakeable {
                    crate::syscalls::futex::wake_process_futex_threads(proc.id);
                    crate::syscalls::futex::wake_process_blocked_threads(proc.id);
                }
            } else {
                let new_usec = remaining_usec - tick_usec;
                it.it_value.tv_sec = (new_usec / 1_000_000) as i64;
                it.it_value.tv_usec = (new_usec % 1_000_000) as i64;
            }
        }
    }
}
