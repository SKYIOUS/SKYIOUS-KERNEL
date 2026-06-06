use crate::Color;
use alloc::ffi::CString;

pub struct Framebuffer {
    pub fd: u64,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

impl Framebuffer {
    pub fn open() -> Option<Self> {
        let path = CString::new("/dev/fb0").ok()?;
        let fd = skyos_libc::syscall::open(path.as_ptr() as *const u8, 2);
        if (fd as i64) < 0 {
            return None;
        }
        Some(Framebuffer {
            fd,
            width: 800,
            height: 600,
            stride: 800,
        })
    }

    pub fn fill_rect(&self, x: usize, y: usize, w: usize, h: usize, color: Color) {
        let mut buf = alloc::vec::Vec::with_capacity(w * 4);
        for _ in 0..w {
            buf.extend_from_slice(&color.0.to_le_bytes());
        }
        for row in y..core::cmp::min(y + h, self.height) {
            let offset = (row * self.stride + x) * 4;
            let _ = skyos_libc::syscall::lseek(self.fd, offset as i64, 0);
            let _ = skyos_libc::syscall::write(self.fd, &buf);
        }
    }

    pub fn set_pixel(&self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.stride + x) * 4;
        let _ = skyos_libc::syscall::lseek(self.fd, offset as i64, 0);
        let pixel = color.0.to_le_bytes();
        let _ = skyos_libc::syscall::write(self.fd, &pixel);
    }

    pub fn blit(&self, data: &[u8]) {
        let _ = skyos_libc::syscall::lseek(self.fd, 0, 0);
        let _ = skyos_libc::syscall::write(self.fd, data);
    }

    pub fn clear(&self, color: Color) {
        self.fill_rect(0, 0, self.width, self.height, color);
    }
}
