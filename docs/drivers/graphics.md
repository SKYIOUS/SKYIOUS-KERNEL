# Framebuffer Graphics Driver

The framebuffer driver provides display output through linear framebuffers.

## UEFI GOP Framebuffer

On UEFI systems, the bootloader sets up a linear framebuffer via the UEFI Graphics Output Protocol (GOP). The kernel receives the framebuffer address, size, stride, and pixel format from the bootloader.

```rust
pub struct Framebuffer {
    pub address: VirtAddr,
    pub width: usize,
    pub height: usize,
    pub stride: usize,       // Bytes per row
    pub bpp: usize,          // Bits per pixel (typically 32)
    pub pixel_format: PixelFormat,
}
```

## Pixel Formats

Supported pixel formats:
- **BGRA32** (0): 32 bits per pixel, 8 bits per channel in BGRA order
- **RGB32** (1): 32 bits per pixel, 8 bits per channel in RGB order
- **RGB565** (2): 16 bits per pixel, 5-6-5 bit RGB channels

## Drawing Operations

The driver provides software-based drawing primitives:

```rust
impl Framebuffer {
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32);
    pub fn blit(&mut self, src: &[u32], x: usize, y: usize, w: usize, h: usize);
    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32);
    pub fn draw_char(&mut self, x: usize, y: usize, c: char, fg: u32, bg: u32);
}
```

## Double Buffering

The driver implements double buffering to avoid tearing:
1. The compositor draws to a back buffer
2. A vertical sync (VSync) signal is detected (or estimated)
3. The back buffer is copied to the front buffer (scanout buffer)
4. The swap is synchronized to the display's refresh rate

## Future GPU Support

Future versions will support:
- Hardware-accelerated blitting via GPU
- Mode setting (resolution and refresh rate changes)
- Multiple monitors
- Hardware cursor overlay
