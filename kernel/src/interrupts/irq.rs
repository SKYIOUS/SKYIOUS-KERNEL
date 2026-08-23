//! Device IRQ handlers: timer, keyboard, mouse, network, TLB flush, IPI.
//!
//! The timer handler drives the scheduler tick and checks for soft lockups.
//! Debug-only diagnostics (mouse state, thread dumps) live in `diag.rs`
//! and are gated behind `#[cfg(debug_assertions)]`.

use core::sync::atomic::Ordering;
use x86_64::structures::idt::InterruptStackFrame;

use super::diag::{soft_lockup_check, diag_first_tick, diag_mouse_state, diag_thread_dump};
use super::TICKS;

pub(super) extern "x86-interrupt" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    let ticks = TICKS.fetch_add(1, Ordering::Release) + 1;

    crate::drivers::watchdog::pet();

    // Debug-only diagnostics: first tick, mouse state, thread dump.
    // All are #[cfg(debug_assertions)] — zero cost in release builds.
    diag_first_tick(ticks);
    diag_mouse_state(ticks);
    diag_thread_dump(ticks, _stack_frame.instruction_pointer.as_u64());

    // Soft-lockup detector: one RIP pinned for 500 consecutive ticks is a
    // busy loop with IF=1 (timer still fires). Always enabled.
    soft_lockup_check(_stack_frame.instruction_pointer.as_u64());

    crate::apic::eoi();

    crate::task::scheduler::tick(ticks);
    crate::task::scheduler::try_schedule();
}

pub(super) extern "x86-interrupt" fn tlb_flush_handler(
    _stack_frame: InterruptStackFrame)
{
    unsafe {
        use x86_64::registers::control::Cr3;
        let (frame, flags) = Cr3::read();
        Cr3::write(frame, flags);
    }
    crate::apic::eoi();
}

pub(super) extern "x86-interrupt" fn mouse_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    crate::drivers::mouse::MOUSE_IRQ_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    loop {
        let mut status_port = Port::<u8>::new(0x64);
        let status = unsafe { status_port.read() };
        if status & 1 == 0 {
            break;
        }
        let mut data_port = Port::<u8>::new(0x60);
        let byte = unsafe { data_port.read() };

        if status & 0x20 != 0 {
            crate::drivers::mouse::feed_byte(byte);
        } else {
            crate::keyboard::handle_scancode(byte);
            crate::tty::feed_scancode(byte);
        }
    }

    // IRQ12 arrives via IOAPIC->LAPIC (vec 44); the PIC is masked, so only the
    // LAPIC EOI clears ISR44. Without it the LAPIC suppresses all class-2
    // vectors (32-47) on this CPU, including the timer (vec 32).
    crate::apic::eoi();
}

pub(super) extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    // One-shot: print on first IRQ1 fire
    static KB_FIRED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if !KB_FIRED.swap(true, core::sync::atomic::Ordering::Relaxed) {
        crate::serial_write("[KBD] IRQ1 fired!\n");
    }

    loop {
        let mut status_port = Port::<u8>::new(0x64);
        let status = unsafe { status_port.read() };
        if status & 1 == 0 {
            break;
        }
        let mut data_port = Port::<u8>::new(0x60);
        let byte = unsafe { data_port.read() };

        if status & 0x20 != 0 {
            crate::drivers::mouse::feed_byte(byte);
        } else {
            crate::keyboard::handle_scancode(byte);
            crate::tty::feed_scancode(byte);
        }
    }

    // IRQ1 arrives via IOAPIC->LAPIC (vec 33); PIC is masked, so only the
    // LAPIC EOI clears ISR33 (same class-2 reasoning as the mouse handler).
    crate::apic::eoi();
}

pub(super) extern "x86-interrupt" fn ipi_func_handler(
    _stack_frame: InterruptStackFrame)
{
    let cpu = crate::syscalls::get_per_cpu();
    let kind = cpu.ipi_kind.swap(0, core::sync::atomic::Ordering::AcqRel);
    match kind {
        1 => {
            // TlbShootdown
            unsafe {
                use x86_64::registers::control::Cr3;
                let (frame, flags) = Cr3::read();
                Cr3::write(frame, flags);
            }
        }
        2 => {
            // Reschedule
            crate::task::scheduler::try_schedule();
        }
        3 => {
            // Func - call registered function pointer (CFI validated)
            let func_val = cpu.ipi_arg.swap(0, core::sync::atomic::Ordering::AcqRel);
            if func_val != 0 {
                if crate::sync::cfi::cfi_check(func_val as usize) {
                    let func: extern "C" fn(u64) = unsafe { core::mem::transmute(func_val) };
                    func(0);
                } else {
                    crate::serial_write("[CFI] Blocked invalid IPI function pointer\n");
                }
            }
        }
        _ => {}
    }
    crate::apic::eoi();
}

pub(super) extern "x86-interrupt" fn network_interrupt_handler(
    _stack_frame: InterruptStackFrame) 
{
    #[cfg(feature = "net")]
    {
        let icr = crate::drivers::net::NIC.lock().as_ref().map(|nic| {
            match nic {
                crate::drivers::net::NicDevice::E1000(dev) => {
                    dev.lock().inner.read_reg(crate::drivers::net::e1000::REG_ICR)
                }
                _ => 0,
            }
        }).unwrap_or(0);

        if icr == 0 {
            crate::apic::eoi();
            return;
        }

        crate::net::poll();
    }
    crate::apic::eoi();
}


