pub mod blend;
pub mod blur;
#[allow(clippy::module_inception)]
pub mod compositor; // hardware-backed compositor impl, mirrors crate/module namespace
pub mod flush;
pub mod scene;
pub mod shadow;
pub mod vsync;

pub use blend::{blend_surface, BlendMode};
pub use blur::GaussianBlur;
pub use compositor::HwCompositor;
pub use flush::{gui_flush_async, poll_flush, FlushResult};
pub use scene::{DirtyRect, GuiScene, GuiWindow};
pub use shadow::{render_shadow, ShadowParams};
pub use vsync::{wait_vsync, FpsCounter};
