//! Interrupt handling for the Vahi kernel.
//!
//! This module is decomposed into focused sub-modules:
//! - `diag` — IRQ-safe formatting utilities and soft-lockup detector
//! - `exceptions` — CPU exception handlers (#BP, #GP, #SS, #UD, #NM, #DF)
//! - `page_fault` — #PF handler with swap/CoW/demand paging
//! - `irq` — Device IRQ handlers (timer, keyboard, mouse, network, TLB, IPI)
//!
//! This file (`mod.rs`) owns the IDT initialization, shared statics, and
//! type definitions that all sub-modules depend on.

pub mod diag;
pub mod exceptions;
pub mod page_fault;
pub mod irq;

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
#[cfg(not(target_arch = "aarch64"))]
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

// ─── PIC constants ──────────────────────────────────────────────

#[cfg(not(target_arch = "aarch64"))]
pub const PIC_1_OFFSET: u8 = 32;
#[cfg(not(target_arch = "aarch64"))]
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[cfg(not(target_arch = "aarch64"))]
// SAFETY: ChainedPics::new is safe when offsets are valid PIC interrupt offsets
pub static PICS: crate::sync::IrqSafeMutex<pic8259::ChainedPics> =
    crate::sync::IrqSafeMutex::new(unsafe { pic8259::ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

// ─── Tick counter ───────────────────────────────────────────────

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Acquire)
}

// ─── Interrupt vector indices ───────────────────────────────────

#[cfg(not(target_arch = "aarch64"))]
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = 32,
    Keyboard = 33,
    _PageFault = 14,
    Mouse = 44,
    Network = 43,
    TlbFlush = 250,
    IpiFunc = 251,
}

#[cfg(not(target_arch = "aarch64"))]
impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

// ─── IDT management ─────────────────────────────────────────────

// ponytail: box-leaked IDT for 'static lifetime; raw ptr for interior mutability
#[cfg(not(target_arch = "aarch64"))]
struct IdtPtr(*mut InterruptDescriptorTable);
#[cfg(not(target_arch = "aarch64"))]
unsafe impl Send for IdtPtr {}
#[cfg(not(target_arch = "aarch64"))]
unsafe impl Sync for IdtPtr {}

#[cfg(not(target_arch = "aarch64"))]
static IDT: crate::sync::IrqSafeMutex<Option<IdtPtr>> = crate::sync::IrqSafeMutex::new(None);

#[cfg(not(target_arch = "aarch64"))]
pub fn init_idt() {
    use alloc::boxed::Box;

    let mut idt = Box::new(InterruptDescriptorTable::new());
    idt.breakpoint.set_handler_fn(exceptions::breakpoint_handler);
    unsafe {
        idt.double_fault.set_handler_fn(exceptions::double_fault_handler)
            .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
    }
    // Route #PF through `vahi_pf_dispatch` (asm): it stashes the entry RSP for
    // `abort_user_copy` before entering the normal Rust handler.
    // SAFETY: trampoline preserves all GPRs + fault-entry stack layout exactly.
    unsafe {
        idt.page_fault.set_handler_addr(x86_64::VirtAddr::new(page_fault::vahi_pf_dispatch as *const () as u64));
    }
    idt.general_protection_fault.set_handler_fn(exceptions::general_protection_fault_handler);
    idt.stack_segment_fault.set_handler_fn(exceptions::stack_segment_fault_handler);
    idt.invalid_opcode.set_handler_fn(exceptions::invalid_opcode_handler);
    idt.device_not_available.set_handler_fn(exceptions::device_not_available_handler);

    idt[InterruptIndex::Timer.as_usize()]
        .set_handler_fn(irq::timer_interrupt_handler);
    idt[InterruptIndex::Keyboard.as_usize()]
        .set_handler_fn(irq::keyboard_interrupt_handler);
    idt[InterruptIndex::Mouse.as_usize()]
        .set_handler_fn(irq::mouse_interrupt_handler);
    idt[InterruptIndex::Network.as_usize()]
        .set_handler_fn(irq::network_interrupt_handler);
    idt[InterruptIndex::TlbFlush.as_usize()]
        .set_handler_fn(irq::tlb_flush_handler);
    idt[InterruptIndex::IpiFunc.as_usize()]
        .set_handler_fn(irq::ipi_func_handler);

    let raw = Box::into_raw(idt);
    // SAFETY: table is box-leaked (into_raw never freed), lives forever
    // load() is safe when IDT is properly configured
    unsafe { (*raw).load(); }
    *IDT.lock() = Some(IdtPtr(raw));

    unsafe {
        let mut pics = PICS.lock();
        pics.write_masks(0xFF, 0xFF);
        pics.initialize();
        pics.write_masks(0xFF, 0xFF);
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn init_ap() {
    if let Some(IdtPtr(ptr)) = *IDT.lock() {
        use x86_64::instructions::tables::{lidt, DescriptorTablePointer};
        use x86_64::VirtAddr;
        // SAFETY: table is box-leaked, never freed
        unsafe {
            let pointer = DescriptorTablePointer {
                base: VirtAddr::from_ptr(ptr as *const InterruptDescriptorTable),
                limit: (core::mem::size_of::<InterruptDescriptorTable>() - 1) as u16,
            };
            lidt(&pointer);
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
type MsiHandler = extern "x86-interrupt" fn(InterruptStackFrame);

#[cfg(not(target_arch = "aarch64"))]
pub fn set_handler(vector: u8, handler: MsiHandler) {
    if let Some(IdtPtr(ptr)) = *IDT.lock() {
        // SAFETY: single-core during registration; idt lives forever
        unsafe { (&mut *ptr)[vector as usize].set_handler_fn(handler); }
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn set_network_vector(vector: u8) {
    set_handler(vector, irq::network_interrupt_handler);
    NET_VECTOR.store(vector, Ordering::Relaxed);
}

#[cfg(not(target_arch = "aarch64"))]
static NET_VECTOR: AtomicU8 = AtomicU8::new(InterruptIndex::Network as u8);

// Re-export `IrqFmtBuf` for use by other crate modules
// (e.g., the old `interrupts::IrqFmtBuf` path used in other files)
pub(crate) use diag::IrqFmtBuf;
