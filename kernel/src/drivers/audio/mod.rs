#![allow(dead_code)]
pub mod hda;
pub mod pcspeaker;

use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;
use alloc::sync::Arc;
use crate::syscalls::errno::Errno;

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

/// Trait for audio output devices
pub trait AudioDevice: Send + Sync {
    fn play_tone(&self, frequency: u32, duration_ms: u32) -> Result<(), Errno>;
    fn stop(&self) -> Result<(), Errno>;
    fn set_volume(&self, volume: u8) -> Result<(), Errno>;
}

/// PC speaker implementation of AudioDevice
pub struct PcSpeaker;

impl AudioDevice for PcSpeaker {
    fn play_tone(&self, frequency: u32, duration_ms: u32) -> Result<(), Errno> {
        if frequency == 0 {
            return Err(Errno::EINVAL);
        }
        pcspeaker::beep(frequency, duration_ms);
        Ok(())
    }

    fn stop(&self) -> Result<(), Errno> {
        // Disable speaker by calling beep with freq=0 (handled in beep)
        pcspeaker::beep(0, 0);
        Ok(())
    }

    fn set_volume(&self, _volume: u8) -> Result<(), Errno> {
        // PC speaker has no volume control
        Ok(())
    }
}

/// Global audio device
pub static AUDIO_DEVICE: Mutex<Option<Arc<dyn AudioDevice>>> = Mutex::new(None);

/// Initialize the audio subsystem — registers the PC speaker by default
pub fn init() {
    *AUDIO_DEVICE.lock() = Some(Arc::new(PcSpeaker));
}

/// Register a custom audio device
pub fn register_audio(device: Arc<dyn AudioDevice>) {
    *AUDIO_DEVICE.lock() = Some(device);
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
