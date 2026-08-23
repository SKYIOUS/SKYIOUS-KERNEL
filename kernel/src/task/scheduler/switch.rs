//! Context switch logic.
//!
//! `prepare_switch` picks the next thread and sets up the switch.
//! `schedule` is the blocking scheduler loop (used by syscalls).
//! `try_schedule` is the non-blocking version (used by timer IRQ and idle loop).

use super::{PerCpuScheduler, SCHED_QUIESCE, this_cpu_sched, route_outgoing, route_switching_old};
use core::sync::atomic::Ordering;

impl PerCpuScheduler {
    /// Caller MUST drop the Mutex guard BEFORE calling switch_context.
    pub fn prepare_switch(&mut self) -> Option<(*mut u64, u64)> {
        // Reclaim anything parked by an earlier switch whose post-switch
        // drain never ran.
        if let Some(parked) = self.switching_old.take() {
            route_outgoing(&mut *self, parked);
        }
        let mut next = match self.pick_next() {
            Some(t) => t,
            None => {
                // Nothing runnable. Switch to idle when current is
                // Exited/empty/blocked-on-queue.
                let want_idle = match self.current_thread.as_ref().map(|t| t.status) {
                    None | Some(crate::task::thread::ThreadStatus::Exited) => true,
                    Some(crate::task::thread::ThreadStatus::Blocked) => {
                        let cur = self.current_thread.as_ref().unwrap();
                        cur.futex_wake_addr.is_some() || cur.pipe_block_key.is_some()
                    }
                    _ => false,
                };
                if want_idle {
                    match self.idle.take() {
                        Some(idle) => idle,
                        None => return None,
                    }
                } else {
                    return None;
                }
            }
        };
        let child_first = next.first_switch_pending;

        // Update CURRENT_PROCESS before activating address space.
        if let Some(ref process) = next.process {
            let mut cur_proto = match crate::task::process::CURRENT_PROCESS.try_lock() {
                Some(guard) => guard,
                None => {
                    let p_idx = core::cmp::min(next.priority as usize, 7);
                    self.ready_queues[p_idx].push_back(next);
                    self.mark_ready_queues_dirty();
                    return None;
                }
            };
            unsafe {
                process.address_space.activate();
            }
            *cur_proto = Some(process.clone());
        }

        next.status = crate::task::thread::ThreadStatus::Running;
        next.first_switch_pending = false;
        let new_rsp = next.stack_ptr;
        let stack_top = next.stack_top();

        let old_rsp_ptr = if let Some(mut old) = self.current_thread.take() {
            if old.status == crate::task::thread::ThreadStatus::Exited {
                // Park dying thread — stack reclaimed on next route_outgoing.
                self.switching_old = Some(old);
                &raw mut self.dummy
            } else if self.idle_id == Some(old._id) {
                // Preempted idle thread: save context, put back in idle slot.
                let p = &mut old.stack_ptr as *mut u64;
                old.pass = old.pass.wrapping_add(old.stride);
                self.idle = Some(old);
                p
            } else {
                let p = &mut old.stack_ptr as *mut u64;
                old.pass = old.pass.wrapping_add(old.stride);
                if child_first {
                    // Fork/clone child: route parent directly.
                    route_outgoing(&mut *self, old);
                } else {
                    self.switching_old = Some(old);
                }
                p
            }
        } else {
            self.switching_old = None;
            &raw mut self.dummy
        };

        self.current_thread = Some(next);
        crate::syscalls::set_kernel_stack(stack_top);
        crate::gdt::set_privilege_stack(stack_top);

        Some((old_rsp_ptr, new_rsp))
    }

    pub fn prepare_switch_tls(&mut self) -> Option<(*mut u64, u64, u64)> {
        let (old, new) = self.prepare_switch()?;
        let fs_base = self.current_thread.as_ref().map(|t| t.fs_base).unwrap_or(0);
        Some((old, new, fs_base))
    }
}

/// Main scheduler loop for each CPU.
///
/// Returns when the current thread is the only runnable work and can resume.
pub fn schedule() {
    let mut watchdog_counter = 0u64;
    loop {
        if SCHED_QUIESCE.load(Ordering::Relaxed) {
            x86_64::instructions::interrupts::enable_and_hlt();
            continue;
        }
        let saved: u64;
        unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) saved, options(att_syntax)); }

        let (old_ptr, new_sp, new_fs) = {
            let mut s = this_cpu_sched().lock();
            s.prepare_switch_tls()
        }.map_or((core::ptr::null_mut(), 0, 0), |(a, b, c)| (a, b, c));

        if !old_ptr.is_null() {
            crate::task::thread::switch_thread(old_ptr, new_sp, new_fs);

            let mut s = this_cpu_sched().lock();
            route_switching_old(&mut s);
            drop(s);
        } else {
            let mut s = this_cpu_sched().lock();
            if let Some(cur) = s.current_thread.as_mut() {
                if cur.status == crate::task::thread::ThreadStatus::Running {
                    drop(s);
                    if saved & 0x200 != 0 {
                        unsafe { core::arch::asm!("sti"); }
                    }
                    return;
                }
                if cur.status == crate::task::thread::ThreadStatus::Blocked {
                    let time_wake = cur.sleep_until
                        .map_or(false, |t| crate::interrupts::get_ticks() >= t);
                    let sig_wake = cur.sleep_until.is_some()
                        && crate::syscalls::check_signal_interrupt();
                    if time_wake || sig_wake {
                        cur.status = crate::task::thread::ThreadStatus::Running;
                        cur.sleep_until = None;
                        drop(s);
                        if saved & 0x200 != 0 {
                            unsafe { core::arch::asm!("sti"); }
                        }
                        return;
                    }
                }
            }
            drop(s);
        }

        if saved & 0x200 != 0 {
            unsafe { core::arch::asm!("sti"); }
        }

        watchdog_counter = watchdog_counter.wrapping_add(1);
        if watchdog_counter & 0xFF == 0 {
            crate::drivers::watchdog::pet();
            crate::drivers::watchdog::check();
        }

        crate::memory::frame_info::drain_deferred();
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

/// Non-blocking version for interrupt handlers.
pub fn try_schedule() {
    if SCHED_QUIESCE.load(Ordering::Relaxed) {
        return;
    }
    let saved: u64;
    unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) saved, options(att_syntax)); }

    let switch = {
        let mut s = this_cpu_sched().try_lock();
        if let Some(ref mut sched) = s {
            if sched.current_thread.is_none() {
                if saved & 0x200 != 0 {
                    unsafe { core::arch::asm!("sti"); }
                }
                return;
            }
            sched.prepare_switch_tls()
        } else {
            None
        }
    };

    if let Some((old_ptr, next_ptr, new_fs)) = switch {
        crate::task::thread::switch_thread(old_ptr, next_ptr, new_fs);

        if let Some(mut sched) = this_cpu_sched().try_lock() {
            route_switching_old(&mut sched);
        }
    }

    if saved & 0x200 != 0 {
        unsafe { core::arch::asm!("sti"); }
    }
}
