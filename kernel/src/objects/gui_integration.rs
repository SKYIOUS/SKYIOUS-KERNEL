use alloc::sync::Arc;
use crate::objects::window_object::WindowObject;
use crate::gui::window::Window;

/// Register a window in the object namespace.
pub fn register_window(window: Window) -> Arc<WindowObject> {
    let obj = WindowObject::new(window);
    let name = alloc::format!("Gui/Window/{}", obj.header.name.lock().clone().unwrap_or_default());
    crate::objects::namespace::OBJECT_NAMESPACE.lock().insert(&name, obj.clone());
    obj
}
