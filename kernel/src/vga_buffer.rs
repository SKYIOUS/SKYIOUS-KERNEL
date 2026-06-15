use core::fmt;
use lazy_static::lazy_static;
use spin::Mutex;
use volatile::Volatile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    _Blue = 1,
    _Green = 2,
    Cyan = 3,
    Red = 4,
    _Magenta = 5,
    _Brown = 6,
    _LightGray = 7,
    _DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    _LightRed = 12,
    _Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct ColorCode(u8);

impl ColorCode {
    fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            0x08 => { // Backspace
                if self.column_position > 0 {
                    self.column_position -= 1;
                    let row = BUFFER_HEIGHT - 1;
                    let col = self.column_position;
                    let blank = ScreenChar {
                        ascii_character: b' ',
                        color_code: self.color_code,
                    };
                    self.buffer.chars[row][col].write(blank);
                }
            }
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                let color_code = self.color_code;
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code,
                });
                self.column_position += 1;
            }
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }

    pub fn clear_screen(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.column_position = 0;
    }

        pub fn _set_color_code(&mut self, color_code: ColorCode) {
        self.color_code = color_code;
    }

    pub fn set_color(&mut self, foreground: Color, background: Color) {
        self.color_code = ColorCode::new(foreground, background);
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // printable ASCII byte or newline
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                // not part of printable ASCII range
                _ => self.write_byte(0xfe),
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

pub const VGA_BUFFER_VIRT: u64 = 0xFFFF_8000_000b_8000;

lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::Yellow, Color::Black),
        buffer: unsafe { &mut *(VGA_BUFFER_VIRT as *mut Buffer) },
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        // Suppress on-screen output during boot splash
        if crate::gui::splash::SPLASH_ACTIVE.load(core::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if crate::drivers::graphics::is_active() {
            crate::drivers::graphics::console::_print(args);
        } else {
            WRITER.lock().write_fmt(args).unwrap();
        }
    });
}

pub fn clear_screen() {
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        if crate::drivers::graphics::is_active() {
            crate::drivers::graphics::console::WRITER.lock().clear_screen();
        } else {
            WRITER.lock().clear_screen();
        }
    });
}

pub fn set_color(foreground: Color, background: Color) {
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        if crate::drivers::graphics::is_active() {
            // Simple mapping for prompt
            let fg = match foreground {
                Color::Black => 0x000000,
                Color::_Blue | Color::LightBlue => 0x0000FF,
                Color::_Green | Color::LightGreen => 0x00FF00,
                Color::Cyan | Color::LightCyan => 0x00FFFF,
                Color::Red | Color::_LightRed => 0xFF0000,
                Color::_Magenta | Color::_Pink => 0xFF00FF,
                Color::_Brown | Color::Yellow => 0xFFFF00,
                Color::_LightGray | Color::_DarkGray | Color::White => 0xFFFFFF,
            };
            let bg = match background {
                Color::Black => 0x001A237E,
                Color::_Blue => 0x001A237E,
                Color::_Green => 0x001A237E,
                Color::Cyan => 0x001A237E,
                Color::Red => 0x001A237E,
                Color::_Magenta => 0x001A237E,
                Color::_Brown => 0x001A237E,
                Color::_LightGray => 0x001A237E,
                Color::_DarkGray => 0x001A237E,
                Color::LightBlue => 0x001A237E,
                Color::LightGreen => 0x001A237E,
                Color::LightCyan => 0x001A237E,
                Color::_LightRed => 0x001A237E,
                Color::_Pink => 0x001A237E,
                Color::Yellow => 0x001A237E,
                Color::White => 0x001A237E,
            };
            crate::drivers::graphics::console::set_console_color(fg, bg);
        } else {
            WRITER.lock().set_color(foreground, background);
        }
    });
}
