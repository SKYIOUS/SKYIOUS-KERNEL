use crate::drivers::gpu::ring::{GpuCommand, GpuCommandRing, GpuOpcode};

pub struct ShadowParams {
    pub radius: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub color: u32,
    pub opacity: f32,
}

#[repr(C, packed)]
struct ShadowRectPayload {
    surface: u32,
    window_x: i32,
    window_y: i32,
    window_w: u32,
    window_h: u32,
    shadow_radius: u32,
    offset_x: i32,
    offset_y: i32,
    color: u32,
    opacity: u32,
}

pub fn render_shadow(
    ring: &GpuCommandRing,
    surface: u32,
    shadow: &ShadowParams,
    window_x: i32, window_y: i32,
    window_w: u32, window_h: u32,
) -> Result<u64, ()> {
    let opacity_fixed = (shadow.opacity.clamp(0.0, 1.0) * 65536.0) as u32;
    let r = shadow.radius;
    let ox = shadow.offset_x;
    let oy = shadow.offset_y;

    // Submit as a single nine-patch shadow command
    let payload = ShadowRectPayload {
        surface,
        window_x, window_y, window_w, window_h,
        shadow_radius: r,
        offset_x: ox, offset_y: oy,
        color: shadow.color,
        opacity: opacity_fixed,
    };
    let cmd = GpuCommand {
        opcode: GpuOpcode::ShadowRect as u32,
        flags: 0, payload_offset: 0,
        payload_len: core::mem::size_of::<ShadowRectPayload>() as u32,
        fence_id: 0, reserved: [0; 8],
    };
    // SAFETY: ShadowRectPayload is repr(C, packed) and pointer-to-byte-slice is valid
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &payload as *const ShadowRectPayload as *const u8,
            core::mem::size_of::<ShadowRectPayload>(),
        )
    };
    ring.submit(&cmd, bytes)
}
