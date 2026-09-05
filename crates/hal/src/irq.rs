//! Interrupt controller abstraction.
//!
//! Provides a trait-based interface for IRQ controllers (APIC, GIC, etc.)
//! and global registration so drivers can route interrupts without
//! depending on a specific hardware implementation.

use alloc::sync::Arc;
use vahi_sync::IrqSafeMutex as Mutex;

/// Hardware IRQ vector number (0-255).
pub type IrqVector = u8;

/// Trigger mode for an interrupt line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqTrigger {
    /// Edge-triggered: fires on voltage transition.
    Edge,
    /// Level-triggered: fires while voltage is asserted.
    Level,
}

/// Signal polarity for an interrupt line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqPolarity {
    /// Active-high (default for most devices).
    ActiveHigh,
    /// Active-low (common for PCI INTx).
    ActiveLow,
}

/// Abstract interrupt controller operations.
///
/// Implemented by the APIC (x86) or GIC (ARM) subsystems.
pub trait InterruptController: Send + Sync {
    /// Send End-Of-Interrupt for the given vector.
    fn eoi(&self, vector: IrqVector);

    /// Mask or unmask an IRQ line.
    fn mask_irq(&self, irq: u8, masked: bool);

    /// Route a PCI IRQ to a specific interrupt vector.
    fn route_pci_irq(&self, bus: u8, device: u8, pin: u8, vector: IrqVector);

    /// Set the CPU affinity mask for an interrupt vector.
    fn set_affinity(&self, vector: IrqVector, cpu_mask: u64);

    /// Get a unique identifier for this controller instance.
    fn controller_id(&self) -> u32;

    /// Enable the interrupt controller on this CPU.
    ///
    /// # Safety
    ///
    /// Must be called with interrupts disabled and only once per CPU during boot.
    unsafe fn enable_cpu(&self);
}

/// Global interrupt controller instance. Set once during boot.
static CURRENT_IRQ_CONTROLLER: Mutex<Option<Arc<dyn InterruptController>>> = Mutex::new(None);

/// Register the system's interrupt controller.
pub fn register_controller(ctrl: Arc<dyn InterruptController>) {
    *CURRENT_IRQ_CONTROLLER.lock() = Some(ctrl);
}

/// Get the registered interrupt controller, if any.
pub fn get_controller() -> Option<Arc<dyn InterruptController>> {
    CURRENT_IRQ_CONTROLLER.lock().clone()
}

/// Send End-Of-Interrupt for the given vector through the registered controller.
pub fn eoi(vector: IrqVector) {
    if let Some(ref ctrl) = *CURRENT_IRQ_CONTROLLER.lock() {
        ctrl.eoi(vector);
    }
}
