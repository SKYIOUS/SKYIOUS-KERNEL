//! # PSF2 Font Loader
//!
//! Provides support for PC Screen Font (PSF) version 2.

#[repr(C, packed)]
struct Psf2Header {
    magic: [u8; 4],
    version: u32,
    header_size: u32,
    flags: u32,
    length: u32,
    char_size: u32,
    height: u32,
    width: u32,
}

pub struct PsfFont {
    header: *const Psf2Header,
    glyphs: *const u8,
}

impl PsfFont {
    pub fn new(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<Psf2Header>() {
            return None;
        }

        let header = data.as_ptr() as *const Psf2Header;
        unsafe {
            if &(*header).magic != b"\x72\xb5\x4a\x86" {
                return None;
            }

            let glyphs = data.as_ptr().add((*header).header_size as usize);
            Some(PsfFont { header, glyphs })
        }
    }

    pub fn draw_char(&self, c: char, x: usize, y: usize, fg: u32, bg: u32, fb: *mut u32, stride: usize) {
        let index = if (c as u32) < unsafe { (*self.header).length } {
            c as u32
        } else {
            0
        };

        unsafe {
            let height = (*self.header).height as usize;
            let width = (*self.header).width as usize;
            let bytes_per_line = (width + 7) / 8;
            let glyph_ptr = self.glyphs.add(index as usize * (*self.header).char_size as usize);

            for cy in 0..height {
                for cx in 0..width {
                    let byte_offset = cy * bytes_per_line + cx / 8;
                    let bit_offset = 7 - (cx % 8);
                    let pixel = (*glyph_ptr.add(byte_offset) >> bit_offset) & 1;
                    
                    let color = if pixel == 1 { fg } else { bg };
                    let fb_offset = (y + cy) * stride + (x + cx);
                    *fb.add(fb_offset) = color;
                }
            }
        }
    }

    pub fn width(&self) -> usize {
        unsafe { (*self.header).width as usize }
    }

    pub fn height(&self) -> usize {
        unsafe { (*self.header).height as usize }
    }
}
