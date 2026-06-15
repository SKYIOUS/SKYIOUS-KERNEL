#![allow(dead_code)]
pub mod hda;
pub mod pcspeaker;

/// Audio subsystem globals and control API.
use spin::Mutex;
use lazy_static::lazy_static;
use alloc::sync::Arc;

lazy_static! {
    pub static ref HDA_DEVICE: Mutex<Option<Arc<Mutex<hda::HdaController>>>> = Mutex::new(None);
}

#[derive(Clone, Copy)]
pub struct VolumeLevel(u8);

impl VolumeLevel {
    pub fn new(percent: u8) -> Self {
        VolumeLevel(if percent > 100 { 100 } else { percent })
    }
    pub fn percent(&self) -> u8 { self.0 }
}

/// Register the detected HDA controller for public API access.
pub fn register_hda(ctrl: hda::HdaController) {
    let ctrl = Arc::new(Mutex::new(ctrl));
    *HDA_DEVICE.lock() = Some(ctrl);
}

/// Set master volume (0-100).
pub fn set_volume(level: VolumeLevel) {
    let dev = HDA_DEVICE.lock();
    if let Some(ref ctrl) = *dev {
        ctrl.lock().set_volume(level.percent());
    }
}

/// Stop audio playback.
pub fn stop_audio() {
    let dev = HDA_DEVICE.lock();
    if let Some(ref ctrl) = *dev {
        ctrl.lock().stop();
    }
}
