//! GuiScene — the bridge between gui's software compositor and HwCompositor.
//! 
//! The gui owns the scene state (windows, damage rects, backbuffer pointer).
//! The HwCompositor owns the HW blit path (shadows, blur, blend, flip).
/// Scene snapshot passed from gui to HwCompositor for each frame.
#[derive(Debug, Clone)]
pub struct GuiScene {
    /// Backbuffer pointer (source pixels for window content)
    pub backbuffer: *const u32,
    /// Screen dimensions
    pub width: u32,
    pub height: u32,
    /// Window surfaces to composite (sorted by z_order, lower first)
    pub windows: alloc::vec::Vec<GuiWindow>,
    /// Merged damage rect for this frame
    pub damage: Option<DirtyRect>,
}

#[derive(Debug, Clone)]
pub struct GuiWindow {
    pub gpu_surface: u32,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub opacity: f32,
    pub blur_radius: u32,
    pub shadow: Option<crate::compositor::shadow::ShadowParams>,
    pub z_order: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DirtyRect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl DirtyRect {
    pub fn from_xywh(x: usize, y: usize, w: usize, h: usize) -> Self {
        DirtyRect { x, y, w, h }
    }
}