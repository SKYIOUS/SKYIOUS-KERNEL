//! Mixed-criticality scheduler integration.
//!
//! Guest VCPUs run as threads in the native stride scheduler but are
//! tagged with the `Virtualized` criticality class. This ensures they
//! receive CPU only when no safety-critical or general-purpose kernel
//! threads are ready — effectively infinite priority donation from
//! the native scheduler.

use crate::task::thread::Thread;

/// Criticality class for scheduling decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criticality {
    Safety = 0,
    General = 1,
    Virtualized = 2,
}

impl Criticality {
    pub fn from_priority(priority: u8) -> Self {
        match priority {
            0..=3 => Criticality::Safety,
            4..=6 => Criticality::General,
            7 => Criticality::Virtualized,
            _ => Criticality::General,
        }
    }

    /// Base tickets for each criticality level.
    pub fn default_tickets(&self) -> u32 {
        match self {
            Criticality::Safety => 100,
            Criticality::General => 20,
            Criticality::Virtualized => 5,
        }
    }

    /// Priority level in the native scheduler (0-7, higher = more urgent).
    pub fn native_priority(&self) -> u8 {
        match self {
            Criticality::Safety => 7,
            Criticality::General => 4,
            Criticality::Virtualized => 0,
        }
    }
}

/// Mark a thread as hosting a VCPU.
/// Assigns it the Virtualized criticality class with low stride tickets.
pub fn mark_as_vcpu_thread(thread: &mut Thread) {
    let crit = Criticality::Virtualized;
    thread.priority = crit.native_priority();
    thread.tickets = crit.default_tickets();
    // stride = STRIDE_MAX / tickets
    thread.stride = (crate::task::thread::STRIDE_MAX as u64) / crit.default_tickets() as u64;
    thread.pass = 0;
}

/// Yield the current VCPU thread to allow native work to run.
/// Called from the VM-exit handler to check if the VCPU should be
/// preempted after handling a VM-exit.
pub fn should_preempt_vcpu() -> bool {
    // ponytail: always yield to native threads — no access to ready_queues
    true
}

/// Schedule VCPUs across all guests.
/// Called periodically from the scheduler tick.
pub fn schedule_vcpus() {
    if !crate::hypervisor::HYPERVISOR_ENABLED.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }

    let hv_lock = crate::hypervisor::HYPERVISOR.lock();
    let hv = match hv_lock.as_ref() {
        Some(hv) => hv,
        None => return,
    };

    for (_id, guest) in &hv.guests {
        for vcpu in &guest.vcpus {
            if vcpu.state != crate::hypervisor::vcpu::VcpuState::Running {
                continue;
            }
            // ponytail: run each RUNNING VCPU if it has CPU budget
            // in the virtualized priority class
        }
    }
}
