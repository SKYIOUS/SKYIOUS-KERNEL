use core::fmt;
use font8x8::UnicodeFonts;
use font8x8::BASIC_FONTS;
use spin::Mutex;
use core::sync::atomic::Ordering;
use crate::drivers::graphics::{FRAMEBUFFER, WIDTH, HEIGHT, STRIDE};
use alloc::vec::Vec;

const SCROLLBACK_LINES: usize = 2048;
const TAB_STOP: usize = 8;

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct CharCell {
    ch: u8,
    fg: u32,
    bg: u32,
}

pub struct ConsoleWriter {
    pub x: usize,
    pub y: usize,
    pub foreground: u32,
    pub background: u32,
    cols: usize,
    rows: usize,
    scrollback: Vec<Vec<CharCell>>,
    scrollback_pos: usize,
    cursor_visible: bool,
    cursor_tick: u64,
    bold: bool,
    blink_state: bool,
    escape_buf: Vec<u8>,
}

impl ConsoleWriter {
    pub fn new() -> Self {
        ConsoleWriter {
            x: 0,
            y: 0,
            foreground: 0xFFFFFFFF,
            background: 0x001A237E,
            cols: 0,
            rows: 0,
            scrollback: Vec::new(),
            scrollback_pos: 0,
            cursor_visible: true,
            cursor_tick: 0,
            bold: false,
            blink_state: true,
            escape_buf: Vec::new(),
        }
    }

    fn init_dims(&mut self) {
        let w = WIDTH.load(Ordering::Relaxed);
        let h = HEIGHT.load(Ordering::Relaxed);
        if w > 0 && h > 0 {
            self.cols = w / 8;
            self.rows = h / 16;
        }
    }

    fn scroll(&mut self) {
        let line_buf: Vec<CharCell> = (0..self.cols)
            .map(|cx| CharCell {
                ch: self.read_cell(cx, 0),
                fg: self.foreground,
                bg: self.background,
            })
            .collect();
        if self.scrollback.len() >= SCROLLBACK_LINES {
            self.scrollback.remove(0);
        }
        self.scrollback.push(line_buf);
        self.scrollback_pos = self.scrollback.len();

        let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
        let stride = STRIDE.load(Ordering::Relaxed);
        if !fb_ptr.is_null() && self.rows > 1 {
            let h = crate::drivers::graphics::HEIGHT.load(core::sync::atomic::Ordering::Relaxed);
            unsafe {
                core::ptr::copy(
                    fb_ptr.add(stride * 16),
                    fb_ptr,
                    (h - 16) * stride,
                );
                for cx in 0..self.cols {
                    for py in 0..16 {
                        for px in 0..8 {
                            let idx = ((self.rows - 1) * 16 + py) * stride + (cx * 8 + px);
                            *fb_ptr.add(idx) = self.background;
                        }
                    }
                }
            }
        }
        if self.rows > 0 {
            self.y = self.rows - 1;
        } else {
            self.y = 0;
        }
    }

    fn read_cell(&self, _cx: usize, _cy: usize) -> u8 {
        b' '
    }

    fn write_char_at(&mut self, cx: usize, cy: usize, c: char, fg: u32, bg: u32) {
        let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
        let stride = STRIDE.load(Ordering::Relaxed);
        let width = WIDTH.load(Ordering::Relaxed);
        let height = HEIGHT.load(Ordering::Relaxed);
        if fb_ptr.is_null() || cx >= self.cols || cy >= self.rows { return; }
        if let Some(glyph) = BASIC_FONTS.get(c) {
            for py in 0..16 {
                for px in 0..8 {
                    let pixel_color = if py < glyph.len() {
                        if (glyph[py] >> px) & 1 == 1 { fg } else { bg }
                    } else { bg };
                    let x_pos = cx * 8 + px;
                    let y_pos = cy * 16 + py;
                    if x_pos < width && y_pos < height {
                        let idx = y_pos * stride + x_pos;
                        unsafe { *fb_ptr.add(idx) = pixel_color; }
                    }
                }
            }
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        if self.cols == 0 || self.rows == 0 { self.init_dims(); }
        match byte {
            b'\n' => self.new_line(),
            b'\r' => { self.x = 0; }
            0x08 => self.backspace(),
            b'\t' => {
                let next_tab = ((self.x / TAB_STOP) + 1) * TAB_STOP;
                if next_tab < self.cols {
                    self.x = next_tab;
                } else {
                    self.new_line();
                    self.x = 0;
                }
            }
            0x1b => {
                self.escape_buf.push(byte);
            }
            0x20..=0x7e => {
                if self.x >= self.cols {
                    self.new_line();
                    self.x = 0;
                }
                let fg = if self.bold { self.brighten(self.foreground) } else { self.foreground };
                self.write_char_at(self.x, self.y, byte as char, fg, self.background);
                self.x += 1;
            }
            _ => {
                if self.x >= self.cols {
                    self.new_line();
                }
                self.write_char_at(self.x, self.y, 0xfe as char, self.foreground, self.background);
                self.x += 1;
            }
        }
    }

    fn process_escape(&mut self) {
        let buf = &self.escape_buf;
        if buf.len() < 2 { self.escape_buf.clear(); return; }
        if buf[0] != 0x1b { self.escape_buf.clear(); return; }

        if buf[1] == b'[' {
            let mut params: Vec<i32> = Vec::new();
            let mut i = 2;
            let mut num: i32 = 0;
            let mut has_num = false;
            let mut command: Option<u8> = None;
            while i < buf.len() {
                let b = buf[i];
                if b >= b'0' && b <= b'9' {
                    num = num * 10 + (b - b'0') as i32;
                    has_num = true;
                } else if b == b';' {
                    params.push(if has_num { num } else { -1 });
                    num = 0;
                    has_num = false;
                } else if b == b'?' {
                    params.push(-2);
                } else if (b >= 0x40 && b <= 0x7e) || (b >= 0x20 && b <= 0x2f) {
                    command = Some(b);
                    if has_num { params.push(num); }
                    break;
                }
                i += 1;
            }
            if let Some(cmd) = command {
                match cmd {
                    b'H' | b'f' => {
                        let row = if params.len() > 0 && params[0] > 0 { params[0] as usize - 1 } else { 0 };
                        let col = if params.len() > 1 && params[1] > 0 { params[1] as usize - 1 } else { 0 };
                        self.y = row.min(self.rows.saturating_sub(1));
                        self.x = col.min(self.cols.saturating_sub(1));
                    }
                    b'J' => {
                        let mode = if params.len() > 0 && params[0] >= 0 { params[0] } else { 0 };
                        if mode == 2 || mode == 3 {
                            self.clear_screen();
                        }
                    }
                    b'K' => {
                        let mode = if params.len() > 0 && params[0] >= 0 { params[0] } else { 0 };
                        match mode {
                            0 => {
                                for cx in self.x..self.cols {
                                    self.write_char_at(cx, self.y, ' ', self.foreground, self.background);
                                }
                            }
                            1 => {
                                for cx in 0..=self.x {
                                    self.write_char_at(cx, self.y, ' ', self.foreground, self.background);
                                }
                            }
                            2 => {
                                for cx in 0..self.cols {
                                    self.write_char_at(cx, self.y, ' ', self.foreground, self.background);
                                }
                            }
                            _ => {}
                        }
                    }
                    b'm' => {
                        if params.is_empty() || (params.len() == 1 && params[0] == 0) {
                            self.foreground = 0xFFFFFFFF;
                            self.background = 0x001A237E;
                            self.bold = false;
                        }
                        for &p in &params {
                            match p {
                                1 => self.bold = true,
                                22 => self.bold = false,
                                30 => self.foreground = 0xFF000000,
                                31 => self.foreground = 0xFFFF0000,
                                32 => self.foreground = 0xFF00FF00,
                                33 => self.foreground = 0xFFFFFF00,
                                34 => self.foreground = 0xFF0000FF,
                                35 => self.foreground = 0xFFFF00FF,
                                36 => self.foreground = 0xFF00FFFF,
                                37 => self.foreground = 0xFFFFFFFF,
                                40 => self.background = 0xFF000000,
                                41 => self.background = 0xFFFF0000,
                                42 => self.background = 0xFF00FF00,
                                43 => self.background = 0xFFFFFF00,
                                44 => self.background = 0xFF0000FF,
                                45 => self.background = 0xFFFF00FF,
                                46 => self.background = 0xFF00FFFF,
                                47 => self.background = 0xFFFFFFFF,
                                _ => {}
                            }
                        }
                    }
                    b'h' => {
                        if params.len() > 0 && params[0] == -2 {
                            self.cursor_visible = true;
                        }
                    }
                    b'l' => {
                        if params.len() > 0 && params[0] == -2 {
                            self.cursor_visible = false;
                        }
                    }
                    _ => {}
                }
            }
        }
        self.escape_buf.clear();
    }

    fn brighten(&self, color: u32) -> u32 {
        let r = ((color >> 16) & 0xFF).min(0x80) * 2;
        let g = ((color >> 8) & 0xFF).min(0x80) * 2;
        let b = (color & 0xFF).min(0x80) * 2;
        (color & 0xFF000000) | (r.min(0xFF) << 16) | (g.min(0xFF) << 8) | b.min(0xFF)
    }

    fn new_line(&mut self) {
        self.x = 0;
        if self.y + 1 >= self.rows {
            self.scroll();
        } else {
            self.y += 1;
        }
    }

    fn backspace(&mut self) {
        if self.x > 0 {
            self.x -= 1;
            self.write_char_at(self.x, self.y, ' ', self.foreground, self.background);
        }
    }

    pub fn clear_screen(&mut self) {
        let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
        let width = WIDTH.load(Ordering::Relaxed);
        let height = HEIGHT.load(Ordering::Relaxed);
        let stride = STRIDE.load(Ordering::Relaxed);
        if fb_ptr.is_null() || width == 0 || height == 0 { return; }

        unsafe {
            for y in 0..height {
                for x in 0..width {
                    *fb_ptr.add(y * stride + x) = self.background;
                }
            }
        }
        self.x = 0;
        self.y = 0;
        self.scrollback.clear();
        self.scrollback_pos = 0;
    }

    pub fn draw_cursor(&mut self) {
        if !self.cursor_visible { return; }
        let cx = self.x;
        let cy = self.y;
        let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
        let stride = STRIDE.load(Ordering::Relaxed);
        let width = WIDTH.load(Ordering::Relaxed);
        if fb_ptr.is_null() || cx >= self.cols || cy >= self.rows { return; }
        let now = crate::interrupts::get_ticks();
        if now.wrapping_sub(self.cursor_tick) >= 10 {
            self.blink_state = !self.blink_state;
            self.cursor_tick = now;
        }
        if !self.blink_state { return; }
        for px in 0..8 {
            for py in 14..16 {
                let x_pos = cx * 8 + px;
                let y_pos = cy * 16 + py;
                if x_pos < width {
                    let idx = y_pos * stride + x_pos;
                    unsafe { *fb_ptr.add(idx) = 0xFFFFFFFF; }
                }
            }
        }
    }

    pub fn set_color(&mut self, foreground: u32, background: u32) {
        self.foreground = foreground;
        self.background = background;
    }
}

impl fmt::Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == 0x1b || !self.escape_buf.is_empty() {
                self.escape_buf.push(byte);
                if byte >= 0x40 && byte <= 0x7e {
                    self.process_escape();
                }
            } else {
                self.write_byte(byte);
            }
        }
        Ok(())
    }
}

pub static WRITER: Mutex<ConsoleWriter> = Mutex::new(ConsoleWriter {
    x: 0,
    y: 0,
    foreground: 0xFFFFFFFF,
    background: 0x001A237E,
    cols: 0,
    rows: 0,
    scrollback: Vec::new(),
    scrollback_pos: 0,
    cursor_visible: true,
    cursor_tick: 0,
    bold: false,
    blink_state: true,
    escape_buf: Vec::new(),
});

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}

pub fn set_console_color(fg: u32, bg: u32) {
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        let mut writer = WRITER.lock();
        writer.set_color(fg, bg);
    });
}
