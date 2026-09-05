//! Timer tick processing.
//!
//! Runs in IRQ context with IF=0 and can preempt any syscall mid-critical-section.
//! A blocking spin here is a permanent deadlock (the preempted holder cannot
//! run until we iret), so every lock is taken non-blocking and contended work
//! is deferred to the next tick.

use super::{this_cpu_sched, GLOBAL};

/// Process timer tick: wake sleeping threads, tick POSIX timers, ITIMER_REAL,
/// accumulate CPU time, expire RR time slices, trigger periodic load balancing,
/// and check memory pressure.
pub fn tick(current_ticks: u64) {
    crate::syscalls::posix_timers::check_posix_timers();
    crate::syscalls::timerfd::check_timerfds();

    // Proactive OOM check: every 100 ticks (~1 second at 100Hz).
    // Detects memory pressure before allocations fail.
    if current_ticks.is_multiple_of(100) {
        crate::task::oom::check_memory_pressure();
    }

    // Phase 1: Scheduler tick — wake sleeping threads, manage RR quanta,
    // accumulate CPU time. Lock is released at end of block.
    {
        let mut sched = match this_cpu_sched().try_lock() {
            Some(s) => s,
            None => return,
        };

        // RR time slice management: decrement quantum for the running RR thread.
        // When it expires, preemption is triggered by try_schedule().
        let mut need_resched = false;
        if let Some(ref mut cur) = sched.current_thread {
            if cur.policy == 2 /* SCHED_RR */ && cur.rr_time_slice > 0 {
                cur.rr_time_slice -= 1;
                if cur.rr_time_slice == 0 {
                    cur.rr_time_slice = 4; // 4 ticks = 40ms at 100 Hz
                    need_resched = true;
                }
            }
        }
        // Preempt if there are pending threads waiting (e.g. fork children).
        // Without this, a CPU-bound SCHED_NORMAL thread never yields to
        // newly spawned threads sitting in the global pending queue.
        if !need_resched {
            if let Some(pq) = GLOBAL.pending_queue.try_lock() {
                if !pq.is_empty() {
                    need_resched = true;
                }
            }
        }
        // Local runnable work: a thread routed onto THIS CPU's ready queues
        // or stride heap (fork-child parent routing, drain_wake, woken
        // sleepers) must be picked even when the global pending queue is
        // empty. Without this an idle CPU with a dirty ready queue never
        // calls pick_next — the only routine that flushes ready_queues —
        // and the thread starves in place forever.
        if !need_resched && !sched.stride_heap.is_empty() {
            need_resched = true;
        }
        if !need_resched
            && sched.ready_queues_dirty
            && sched.ready_queues.iter().any(|q| !q.is_empty())
        {
            need_resched = true;
        }
        // Wake due sleepers / accumulate CPU time FIRST so sleepers wake on
        // their exact tick even when a reschedule is also due — the old early
        // return skipped run_tick_inner on resched ticks, deferring wakes by
        // a whole tick (selftests assert tick-exact wakeup).
        run_tick_inner(&mut sched, current_ticks);
        if need_resched {
            // Drop the scheduler lock before try_schedule — it uses try_lock
            // internally and would always fail if we held the lock.
            drop(sched);
            crate::task::scheduler::try_schedule();
            return;
        }
    } // sched lock released here

    // Phase 2: Load balancing (every 10 ticks, no scheduler lock held)
    if current_ticks.is_multiple_of(10) {
        super::PerCpuScheduler::load_balance();
    }

    // Phase 3: ITIMER_REAL processing (no scheduler lock held)
    tick_itimers();
}

fn run_tick_inner(
    sched: &mut crate::sync::IrqSafeMutexGuard<'_, super::PerCpuScheduler>,
    current_ticks: u64,
) {
    if let Some(mut sleep) = GLOBAL.sleep_queue.try_lock() {
        // Rotate in place instead of draining into a new VecDeque: queue
        // growth allocates, and we are in IRQ context (IF=0).
        let n = sleep.len();
        let mut woken = 0u32;
        for _ in 0..n {
            let Some(mut thread) = sleep.pop_front() else {
                break;
            };
            let mut wake = false;
            if let Some(wake_time) = thread.sleep_until {
                if current_ticks >= wake_time {
                    wake = true;
                }
            }
            if !wake {
                if let Some(ref proc) = thread.process {
                    if let Some(sig) = proc.signals.try_lock() {
                        if sig.has_unmasked_pending(sig.blocked) {
                            wake = true;
                        }
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
        if woken > 0 {
            sched.mark_ready_queues_dirty();
        }
    }

    // Accumulate CPU time for current thread's process
    if let Some(ref cur) = sched.current_thread {
        if let Some(ref proc) = cur.process {
            proc.utime
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Process ITIMER_REAL for every process. Runs after the scheduler lock
/// is released to avoid holding two locks simultaneously.
fn tick_itimers() {
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
            if itimer_count >= itimer_pids.len() {
                break;
            }
            itimer_pids[itimer_count] = *pid;
            itimer_count += 1;
        }
    }
    for &pid in &itimer_pids[..itimer_count] {
        // Clone proc under the guard, then drop it: the SIGALRM routing below
        // re-locks PROCESS_TABLE, and a nested acquisition in IRQ context
        // self-deadlocks the non-reentrant mutex.
        let proc = {
            let table = match crate::task::process::PROCESS_TABLE.try_lock() {
                Some(t) => t,
                None => return,
            };
            table.get(&pid).cloned()
        };
        let Some(proc) = proc else {
            continue;
        };
        let mut it = match proc.itimer_real.try_lock() {
            Some(it) => it,
            None => continue,
        };
        if it.it_value.tv_sec > 0 || it.it_value.tv_usec > 0 {
            let tick_usec = 10_000u64; // 10ms per tick
            let remaining_usec =
                (it.it_value.tv_sec as u64) * 1_000_000 + it.it_value.tv_usec as u64;
            if remaining_usec <= tick_usec {
                // Timer expired
                it.it_value = it.it_interval; // reload
                if let Some(mut sig) = proc.signals.try_lock() {
                    sig.raise(crate::syscalls::signal::Signal::SIGALRM);
                }
                // Route SIGALRM to signalfd instances. IRQ context (I4): the
                // try-lock _for variant never blocks — proc is already held.
                crate::task::process::route_signal_to_signalfd_for(
                    &proc,
                    14,
                    crate::task::process::SI_TIMER,
                    0,
                    0,
                    0,
                );
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
