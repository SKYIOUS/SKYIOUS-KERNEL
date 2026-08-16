// ---------------------------------------------------------------------------
// APIC Unit Tests
// ---------------------------------------------------------------------------

use crate::apic::msi;
use crate::apic::{self, priority};

/// Test MSI vector allocation
pub fn test_msi_alloc() -> Result<(), &'static str> {
    msi::init();
    let v = msi::alloc().ok_or("MSI alloc failed")?;
    if v < 0x50 || v > 0xFE {
        return Err("MSI vector out of range");
    }
    Ok(())
}

/// Test MSI vector range
pub fn test_msi_alloc_range() -> Result<(), &'static str> {
    msi::init();
    let v = msi::alloc().ok_or("MSI alloc failed")?;
    assert!(v >= 0x50, "vector too low: {}", v);
    Ok(())
}

/// Test APIC mode detection
pub fn test_apic_mode_detect() -> Result<(), &'static str> {
    let mode = apic::ApicMode::detect();
    match mode {
        apic::ApicMode::Xapic | apic::ApicMode::X2Apic => Ok(()),
    }
}

/// Test TPR get/set
pub fn test_set_tpr() -> Result<(), &'static str> {
    apic::set_tpr(priority::DEVICE);
    let tpr = apic::tpr();
    if tpr != priority::DEVICE {
        return Err("TPR not set correctly");
    }
    apic::set_tpr(0);
    Ok(())
}
