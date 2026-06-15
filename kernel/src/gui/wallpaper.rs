use alloc::vec::Vec;

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

struct BmpHeader {
    width: u32,
    height: u32,
    bpp: u16,
    pixel_offset: u32,
}

fn parse_bmp_header(data: &[u8]) -> Option<BmpHeader> {
    if data.len() < 54 || data[0] != b'B' || data[1] != b'M' { return None; }
    let pixel_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]);
    let header_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
    if header_size < 12 { return None; }
    let width = u32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height_raw = u32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    let height = height_raw & 0x7FFFFFFF;
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    if bpp != 24 && bpp != 32 { return None; }
    let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
    if compression != 0 { return None; }
    Some(BmpHeader { width, height, bpp, pixel_offset })
}

pub fn decode_bmp(data: &[u8]) -> Option<DecodedImage> {
    let hdr = parse_bmp_header(data)?;
    let w = hdr.width as usize;
    let h = hdr.height as usize;
    if w == 0 || h == 0 || w > 3840 || h > 2160 { return None; }

    let bpp = hdr.bpp as usize;
    let stride = ((w * bpp + 31) / 32) * 4;
    let bottom_up = data[22] & 0x80 == 0;

    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let src_y = if bottom_up { h - 1 - y } else { y };
        let row_start = hdr.pixel_offset as usize + src_y * stride;
        for x in 0..w {
            let px = row_start + x * (bpp / 8);
            if px + 2 >= data.len() { pixels.push(0xFF000000); continue; }
            let b = data[px] as u32;
            let g = data[px + 1] as u32;
            let r = data[px + 2] as u32;
            let a = if bpp == 32 { data[px + 3] as u32 } else { 0xFF };
            pixels.push((a << 24) | (r << 16) | (g << 8) | b);
        }
    }
    Some(DecodedImage { width: hdr.width, height: hdr.height, pixels })
}

pub fn scale_to_screen(img: &DecodedImage, screen_w: usize, screen_h: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(screen_w * screen_h);
    for dy in 0..screen_h {
        for dx in 0..screen_w {
            let sx = (dx as u64 * img.width as u64 / screen_w as u64) as u32;
            let sy = (dy as u64 * img.height as u64 / screen_h as u64) as u32;
            let src_idx = (sy * img.width + sx) as usize;
            let color = if src_idx < img.pixels.len() { img.pixels[src_idx] } else { 0xFF000000 };
            out.push(color);
        }
    }
    out
}
