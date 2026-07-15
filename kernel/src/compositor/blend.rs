use crate::drivers::gpu::ring::{GpuCommand, GpuCommandRing, GpuOpcode};

#[repr(u32)]
pub enum BlendMode {
    Normal = 0,
    Additive = 1,
    Multiply = 2,
    Screen = 3,
    Overlay = 4,
}

#[repr(C, packed)]
struct BlendPayload {
    dst_surface: u32,
    src_surface: u32,
    dst_x: i32,
    dst_y: i32,
    src_x: i32,
    src_y: i32,
    width: u32,
    height: u32,
    alpha: u32,
    blend_mode: u32,
}

pub fn blend_surface(
    ring: &GpuCommandRing,
    dst: u32, src: u32,
    pos: (i32, i32),
    alpha: f32,
    blend_mode: BlendMode,
) -> Result<u64, ()> {
    let alpha_fixed = (alpha.clamp(0.0, 1.0) * 65536.0) as u32;
    let payload = BlendPayload {
        dst_surface: dst, src_surface: src,
        dst_x: pos.0, dst_y: pos.1,
        src_x: 0, src_y: 0,
        width: 0, height: 0,
        alpha: alpha_fixed, blend_mode: blend_mode as u32,
    };
    let cmd = GpuCommand {
        opcode: GpuOpcode::BlendRects as u32,
        flags: 0, payload_offset: 0,
        payload_len: core::mem::size_of::<BlendPayload>() as u32,
        fence_id: 0, reserved: [0; 8],
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &payload as *const BlendPayload as *const u8,
            core::mem::size_of::<BlendPayload>(),
        )
    };
    ring.submit(&cmd, bytes)
}
