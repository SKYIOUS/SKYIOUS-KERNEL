use crate::drivers::gpu::ring::{GpuCommand, GpuCommandRing, GpuOpcode};

const BLUR_KERNEL_MAX: usize = 64;

fn fast_exp(x: f32) -> f32 {
    // ponytail: polynomial exp approximation, precise enough for blur kernels
    let x = x.clamp(-10.0, 0.0);
    let y = 1.0 + x * (1.0 + x * (0.5 + x * (1.0/6.0 + x * (1.0/24.0 + x * (1.0/120.0 + x / 720.0)))));
    y.clamp(0.0, 1.0)
}

pub struct GaussianBlur {
    pub kernel: [f32; BLUR_KERNEL_MAX],
    pub kernel_size: u32,
    pub radius: u32,
}

impl GaussianBlur {
    pub fn generate_kernel(radius: u32) -> Self {
        let r = radius.min((BLUR_KERNEL_MAX / 2) as u32 - 1) as f32;
        let sigma = r / 3.0;
        let size = (2.0 * r + 1.0) as u32;
        let mut kernel = [0.0f32; BLUR_KERNEL_MAX];
        let mut sum = 0.0f32;
        for i in 0..size {
            let x = i as f32 - r;
            let val = fast_exp(-0.5 * (x / sigma) * (x / sigma));
            kernel[i as usize] = val;
            sum += val;
        }
        if sum > 0.0 {
            for i in 0..size {
                kernel[i as usize] /= sum;
            }
        }
        GaussianBlur { kernel, kernel_size: size, radius }
    }

    pub fn apply(
        &self,
        ring: &GpuCommandRing,
        surface: u32,
        x: u32, y: u32, w: u32, h: u32,
    ) -> Result<u64, ()> {
        #[repr(C, packed)]
        struct BlurPayload {
            surface: u32,
            x: u32, y: u32, w: u32, h: u32,
            radius: u32,
            kernel_size: u32,
        }
        let payload = BlurPayload {
            surface, x, y, w, h,
            radius: self.radius,
            kernel_size: self.kernel_size,
        };
        let cmd = GpuCommand {
            opcode: GpuOpcode::BlurRect as u32,
            flags: 0, payload_offset: 0,
            payload_len: core::mem::size_of::<BlurPayload>() as u32,
            fence_id: 0, reserved: [0; 8],
        };
        // SAFETY: BlurPayload is repr(C, packed) — the pointer-to-byte-slice cast is valid
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &payload as *const BlurPayload as *const u8,
                core::mem::size_of::<BlurPayload>(),
            )
        };
        ring.submit(&cmd, bytes)
    }
}
