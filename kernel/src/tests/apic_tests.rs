// ---------------------------------------------------------------------------
// APIC Unit Tests
// ---------------------------------------------------------------------------

use crate::apic::msi;
use crate::apic::{self, priority};
use crate::selftest;

/// Test MSI vector allocation
fn test_msi_alloc() -> Result<(), &'static str> {
    msi::init();
    let v = msi::alloc().ok_or("MSI alloc failed")?;
    if v < 0x50 || v > 0xFE {
        return Err("MSI vector out of range");
    }
    Ok(())
}

/// Test MSI vector range
fn test_msi_alloc_range() -> Result<(), &'static str> {
    msi::init();
    let v = msi::alloc().ok_or("MSI alloc failed")?;
    assert!(v >= 0x50, "vector too low: {}", v);
    Ok(())
}

/// Test APIC mode detection
fn test_apic_mode_detect() -> Result<(), &'static str> {
    let mode = apic::ApicMode::detect();
    match mode {
        apic::ApicMode::Xapic | apic::ApicMode::X2Apic => Ok(()),
    }
}

/// Test TPR get/set
fn test_set_tpr() -> Result<(), &'static str> {
    apic::set_tpr(priority::DEVICE);
    let tpr = apic::tpr();
    if tpr != priority::DEVICE {
        return Err("TPR not set correctly");
    }
    apic::set_tpr(0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register() {
    selftest::register("apic::msi_alloc", test_msi_alloc);
    selftest::register("apic::msi_alloc_range", test_msi_alloc_range);
    selftest::register("apic::mode_detect", test_apic_mode_detect);
    selftest::register("apic::set_tpr", test_set_tpr);
}
