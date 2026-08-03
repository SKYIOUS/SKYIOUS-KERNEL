use alloc::sync::Arc;
use crate::sync::IrqSafeMutex as Mutex;
use crate::objects::{KernelObject, ObjectHeader, security::SecurityDescriptor, TYPE_GUI_WINDOW};
use crate::gui::window::Window;

pub struct WindowObject {
    pub header: ObjectHeader,
    pub inner: Arc<Mutex<Window>>,
}

impl WindowObject {
    pub fn new(window: Window) -> Arc<Self> {
        let sec = SecurityDescriptor::new(0, 0, 0o666);
        let header = ObjectHeader::new(TYPE_GUI_WINDOW, sec);
        *header.name.lock() = Some(alloc::string::String::from(window.title));
        Arc::new(WindowObject {
            header,
            inner: Arc::new(Mutex::new(window)),
        })
    }
}

impl KernelObject for WindowObject {
    fn header(&self) -> &ObjectHeader { &self.header }
    fn type_name(&self) -> &'static str { "GuiWindow" }
    fn query_name(&self) -> Option<alloc::string::String> {
        self.header.name.lock().clone()
    }
    fn ioctl(&self, request: u64, argp: *mut u8) -> Result<u64, ()> {
        match request {
            1 => {
                let win = self.inner.lock();
                let pos = (win.x as u64) | ((win.y as u64) << 16);
                Ok(pos)
            }
            2 => {
                // SAFETY: caller guarantees argp points to two valid u32 values (width, height)
                let w = unsafe { *(argp as *const u32) };
                let h = unsafe { *(argp.add(4) as *const u32) };
                let mut win = self.inner.lock();
                win.width = w as usize;
                win.height = h as usize;
                Ok(0)
            }
            3 => {
                let mut win = self.inner.lock();
                win.dirty = true;
                Ok(0)
            }
            _ => Err(()),
        }
    }
    fn poll_readable(&self) -> bool {
        let win = self.inner.lock();
        !win.key_events.is_empty()
    }
    fn on_close(&self) {
        let mut comp = crate::gui::COMPOSITOR.lock();
        let name = self.header.name.lock().clone().unwrap_or_default();
        comp.windows.retain(|w| w.title != name.as_str());
    }
}
