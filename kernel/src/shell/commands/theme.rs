use crate::println;
use crate::vga_buffer;

pub fn theme(name: &str) {
    if name.is_empty() { return; }
    match name {
        "matrix" => {
            crate::drivers::graphics::console::set_console_color(0x00FF00, 0x000000);
            vga_buffer::clear_screen();
            println!("Wake up, Neo...");
        }
        "vahi" => {
            crate::drivers::graphics::console::set_console_color(0xFFFFFFFF, 0x001A237E);
            vga_buffer::clear_screen();
            println!("Vahi Classic Theme restored.");
        }
        "cyberpunk" => {
            crate::drivers::graphics::console::set_console_color(0xFF00FF, 0x2D002D);
            vga_buffer::clear_screen();
            println!("Cyberpunk mode engaged.");
        }
         "synthwave" => {
            crate::drivers::graphics::console::set_console_color(0x00FFFF, 0x120024);
            vga_buffer::clear_screen();
            println!("Synthwave mode engaged.");
        }
        _ => {
            println!("Unknown theme. Try: vahi, matrix, cyberpunk, synthwave");
        }
    }
}
