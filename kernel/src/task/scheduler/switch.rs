//! Context switch logic.
//!
//! `prepare_switch` picks the next thread and sets up the switch.
//! `schedule` is the blocking scheduler loop (used by syscalls).
//! `try_schedule` is the non-blocking version (used by timer IRQ and idle loop).

use super::{PerCpuScheduler, SCHED_QUIESCE, this_cpu_sched, route_outgoing, route_switching_old, should_preempt};
use core::sync::atomic::Ordering;

// ─── Architecture-specific interrupt helpers ────────────────────

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn save_and_cli() -> u64 {
    let flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) flags, options(att_syntax)); }
    flags
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn restore_if(flags: u64) {
    if flags & 0x200 != 0 {
        unsafe { core::arch::asm!("sti"); }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn save_and_cli() -> u64 {
    let daif: u64;
    unsafe { core::arch::asm!("mrs {0}, daif; msr daifset, #2", out(reg) daif); }
    daif
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn restore_if(daif: u64) {
    unsafe { core::arch::asm!("msr daif, {0}", in(reg) daif); }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn hlt() {
    x86_64::instructions::interrupts::enable_and_hlt();
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn hlt() {
    unsafe { core::arch::asm!("wfi"); }
}

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

        // Preemption check: skip the switch if the current thread outranks
        // the candidate (current is RT, candidate is SCHED_NORMAL/BATCH).
        // should_preempt(A, B) = "should B preempt A" = B is RT, A is not.
        // So should_preempt(next, cur) = cur is RT, next is not → skip.
        if !child_first {
            if let Some(ref cur) = self.current_thread {
                if cur.status == crate::task::thread::ThreadStatus::Running
                    && should_preempt(next.sched_class, cur.sched_class)
                {
                    // Current outranks candidate — put candidate back.
                    let p_idx = core::cmp::min(next.priority as usize, 7);
                    self.ready_queues[p_idx].push_back(next);
                    self.ready_queues_dirty = true;
                    return None;
                }
            }
        }

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

        // Lazily allocate FPU state buffer for the incoming thread (x86_64 only)
        #[cfg(target_arch = "x86_64")]
        if next.fpu_state.is_none() {
            next.fpu_state = Some(alloc::boxed::Box::new(crate::task::thread::FpuArea::new()));
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

    #[cfg(target_arch = "x86_64")]
    pub fn prepare_switch_tls(&mut self) -> Option<(*mut u64, u64, u64, *mut crate::task::thread::FpuArea, *const crate::task::thread::FpuArea)> {
        let old_fpu_ptr = self.current_thread.as_ref()
            .and_then(|t| t.fpu_state.as_ref())
            .map(|b| b.as_ref() as *const _ as *mut crate::task::thread::FpuArea)
            .unwrap_or(core::ptr::null_mut());
        let (old, new) = self.prepare_switch()?;
        let cur = self.current_thread.as_ref()?;
        let fs_base = cur.fs_base;
        let new_fpu = cur.fpu_state.as_ref()
            .map(|b| b.as_ref() as *const _ as *const crate::task::thread::FpuArea)
            .unwrap_or(core::ptr::null());
        Some((old, new, fs_base, old_fpu_ptr, new_fpu))
    }

    #[cfg(target_arch = "aarch64")]
    pub fn prepare_switch_tls(&mut self) -> Option<(*mut u64, u64, u64)> {
        let (old, new) = self.prepare_switch()?;
        let fs_base = self.current_thread.as_ref()?.fs_base;
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
            hlt();
            continue;
        }
        let saved = save_and_cli();

        #[cfg(target_arch = "x86_64")]
        let (old_ptr, new_sp, new_fs, old_fpu, new_fpu) = {
            let mut s = this_cpu_sched().lock();
            s.prepare_switch_tls()
        }.map_or((core::ptr::null_mut(), 0, 0, core::ptr::null_mut(), core::ptr::null()), |(a, b, c, d, e)| (a, b, c, d, e));
        #[cfg(target_arch = "aarch64")]
        let (old_ptr, new_sp, new_fs) = {
            let mut s = this_cpu_sched().lock();
            s.prepare_switch_tls()
        }.map_or((core::ptr::null_mut(), 0, 0), |(a, b, c)| (a, b, c));

        if !old_ptr.is_null() {
            #[cfg(target_arch = "x86_64")]
            crate::task::thread::switch_thread(old_ptr, new_sp, new_fs, old_fpu, new_fpu);
            #[cfg(target_arch = "aarch64")]
            crate::hal::cpu::switch_thread(old_ptr, new_sp, new_fs);

            let mut s = this_cpu_sched().lock();
            route_switching_old(&mut s);
            drop(s);
        } else {
            let mut s = this_cpu_sched().lock();
            if let Some(cur) = s.current_thread.as_mut() {
                if cur.status == crate::task::thread::ThreadStatus::Running {
                    drop(s);
                    restore_if(saved);
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
                        restore_if(saved);
                        return;
                    }
                }
            }
            drop(s);
        }

        restore_if(saved);

        watchdog_counter = watchdog_counter.wrapping_add(1);
        if watchdog_counter & 0xFF == 0 {
            crate::drivers::watchdog::pet();
            crate::drivers::watchdog::check();
        }

        crate::memory::frame_info::drain_deferred();
        hlt();
    }
}

/// Non-blocking version for interrupt handlers.
#[cfg(target_arch = "x86_64")]
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
                restore_if(saved);
                return;
            }
            sched.prepare_switch_tls()
        } else {
            None
        }
    };

    if let Some((old_ptr, next_ptr, new_fs, old_fpu, new_fpu)) = switch {
        crate::task::thread::switch_thread(old_ptr, next_ptr, new_fs, old_fpu, new_fpu);

        if let Some(mut sched) = this_cpu_sched().try_lock() {
            route_switching_old(&mut sched);
        }
    }

    restore_if(saved);
}

#[cfg(target_arch = "aarch64")]
pub fn try_schedule() {
    if SCHED_QUIESCE.load(Ordering::Relaxed) {
        return;
    }
    // aarch64: DAIF flags are in PSTATE; save and disable IRQs
    let saved: u64;
    unsafe { core::arch::asm!("mrs {0}, daif; msr daifset, #2", out(reg) saved); }

    let switch = {
        let mut s = this_cpu_sched().try_lock();
        if let Some(ref mut sched) = s {
            if sched.current_thread.is_none() {
                unsafe { core::arch::asm!("msr daif, {0}", in(reg) saved); }
                return;
            }
            sched.prepare_switch_tls()
        } else {
            None
        }
    };

    if let Some((old_ptr, next_ptr, new_fs)) = switch {
        crate::hal::cpu::switch_thread(old_ptr, next_ptr, new_fs);

        if let Some(mut sched) = this_cpu_sched().try_lock() {
            route_switching_old(&mut sched);
        }
    }

    unsafe { core::arch::asm!("msr daif, {0}", in(reg) saved); }
}
