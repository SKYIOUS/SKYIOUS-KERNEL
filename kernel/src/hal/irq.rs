use spin::Mutex;
use alloc::sync::Arc;

pub type IrqVector = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqTrigger {
    Edge,
    Level,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqPolarity {
    ActiveHigh,
    ActiveLow,
}

pub trait InterruptController: Send + Sync {
    fn eoi(&self, vector: IrqVector);
    fn mask_irq(&self, irq: u8, masked: bool);
    fn route_pci_irq(&self, bus: u8, device: u8, pin: u8, vector: IrqVector);
    fn controller_id(&self) -> u32;
    unsafe fn enable_cpu(&self);
}

static CURRENT_IRQ_CONTROLLER: Mutex<Option<Arc<dyn InterruptController>>> = Mutex::new(None);

pub fn register_controller(ctrl: Arc<dyn InterruptController>) {
    *CURRENT_IRQ_CONTROLLER.lock() = Some(ctrl);
}

pub fn get_controller() -> Option<Arc<dyn InterruptController>> {
    CURRENT_IRQ_CONTROLLER.lock().clone()
}

pub fn eoi(vector: IrqVector) {
    if let Some(ref ctrl) = *CURRENT_IRQ_CONTROLLER.lock() {
        ctrl.eoi(vector);
    }
}
