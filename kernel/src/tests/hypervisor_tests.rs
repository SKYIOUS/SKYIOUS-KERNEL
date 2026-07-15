use crate::hypervisor;

pub fn register() {
    crate::selftest::register("hypervisor_vmx_available", hypervisor_vmx_available);
}

fn hypervisor_vmx_available() -> Result<(), &'static str> {
    if hypervisor::is_virtualization_available() {
        Ok(())
    } else {
        Err("VMX not available on this CPU (expected in QEMU)")
    }
}
