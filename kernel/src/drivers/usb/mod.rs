pub mod xhci;
#[cfg(feature = "uhci")]
pub mod uhci;

pub fn init() {
    crate::println!("USB: Initializing USB Stack...");
    // Future: USB Bus enumeration
}
