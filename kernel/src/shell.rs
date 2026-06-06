pub mod commands;

use crate::println;
use crate::print;
use crate::vga_buffer::{self, Color};
use alloc::string::String;
use alloc::vec::Vec;

pub struct Shell {
    command_buffer: String,
    history: Vec<String>,
    history_index: usize,
}

impl Shell {
    pub fn new() -> Self {
        Shell {
            command_buffer: String::with_capacity(128),
            history: Vec::new(),
            history_index: 0,
        }
    }

    pub fn prompt(&self) {
        vga_buffer::set_color(Color::LightBlue, Color::Black);
        print!("vahi> ");
        vga_buffer::set_color(Color::White, Color::Black);
    }

    pub fn handle_char(&mut self, c: char) {
        match c {
            '\n' => {
                println!("");
                self.execute_command();
                let cmd = self.command_buffer.trim();
                if !cmd.is_empty() {
                    self.history.push(String::from(cmd));
                    if self.history.len() > 100 {
                        self.history.remove(0);
                    }
                }
                self.command_buffer.clear();
                self.history_index = self.history.len();
                self.prompt();
            }
            '\u{0008}' => { // Backspace
                if !self.command_buffer.is_empty() {
                    self.command_buffer.pop();
                    print!("\u{0008}");
                }
            }
            _ => {
                if self.command_buffer.len() < 128 {
                    self.command_buffer.push(c);
                    print!("{}", c);
                }
            }
        }
    }

    pub fn handle_raw_key(&mut self, key: pc_keyboard::KeyCode) {
        match key {
            pc_keyboard::KeyCode::ArrowUp => {
                if self.history_index > 0 {
                    self.history_index -= 1;
                    self.replace_buffer_with_history();
                }
            }
            pc_keyboard::KeyCode::ArrowDown => {
                if self.history_index < self.history.len() {
                    self.history_index += 1;
                    if self.history_index == self.history.len() {
                        self.clear_line_and_buffer();
                    } else {
                        self.replace_buffer_with_history();
                    }
                }
            }
            _ => {}
        }
    }

    fn replace_buffer_with_history(&mut self) {
        self.clear_line_on_screen();
        self.command_buffer = self.history[self.history_index].clone();
        print!("{}", self.command_buffer);
    }

    fn clear_line_and_buffer(&mut self) {
        self.clear_line_on_screen();
        self.command_buffer.clear();
    }

    fn clear_line_on_screen(&mut self) {
        for _ in 0..self.command_buffer.len() {
            print!("\u{0008}");
        }
    }

    pub async fn run(&mut self) {
        use futures_util::stream::StreamExt;
        use crate::task::keyboard::ScancodeStream;
        use pc_keyboard::{Keyboard, layouts, ScancodeSet1, HandleControl, DecodedKey};

        let mut scancodes = ScancodeStream::new();
        let mut keyboard = Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore);

        self.prompt();

        while let Some(scancode) = scancodes.next().await {
            if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
                if let Some(key) = keyboard.process_keyevent(key_event) {
                    match key {
                        DecodedKey::Unicode(character) => {
                            self.handle_char(character);
                        }
                        DecodedKey::RawKey(key) => {
                            self.handle_raw_key(key);
                        }
                    }
                }
            }
        }
    }

    fn execute_command(&self) {
        let command = self.command_buffer.trim();
        if command.is_empty() { return; }

        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();

        match cmd {
            "help" => commands::system::help(),
            "info" => commands::system::info(),
            "uptime" => commands::system::uptime(),
            "clear" => commands::system::clear(),
            "reboot" => commands::system::reboot(),
            "poweroff" => commands::system::poweroff(),
            "neofetch" => commands::system::neofetch(),
            "sleep" => commands::system::sleep(args.get(0).unwrap_or(&"0")),
            "exec" => commands::system::exec(args.get(0).unwrap_or(&"")),
            "kor" => commands::system::kor(args.get(0).unwrap_or(&"")),

            "ls" => commands::fs::ls(args.get(0).unwrap_or(&".")),
            "cd" => commands::fs::cd(args.get(0).unwrap_or(&"")),
            "pwd" => commands::fs::pwd(),
            "mkdir" => commands::fs::mkdir(args.get(0).unwrap_or(&"")),
            "rm" => commands::fs::rm(args.get(0).unwrap_or(&"")),
            "touch" => commands::fs::touch(args.get(0).unwrap_or(&"")),
            "cat" => commands::fs::cat(args.get(0).unwrap_or(&"")),
            "stat" => commands::fs::stat(args.get(0).unwrap_or(&"")),
            "cp" => commands::fs::cp(args.get(0).unwrap_or(&""), args.get(1).unwrap_or(&"")),
            "mount" => commands::fs::mount(),

            #[cfg(feature = "net")]
            "ping" => commands::net::ping(args.get(0).unwrap_or(&"")),
            #[cfg(feature = "net")]
            "nslookup" => commands::net::nslookup(args.get(0).unwrap_or(&"")),
            #[cfg(feature = "net")]
            "fetch" => commands::net::fetch(args.get(0).unwrap_or(&"")),

            "heap_test" => commands::debug::heap_test(),
            "lspci" => commands::debug::lspci(),
            "panic" => commands::debug::panic(),
            "test_pf" => commands::debug::test_pf(),
            "test_cow" => commands::debug::test_cow(),

            #[cfg(feature = "ai_rule")]
            "vahiai" => commands::ai::vahiai(&args),

            "theme" => commands::theme::theme(args.get(0).unwrap_or(&"")),

            _ => {
                vga_buffer::set_color(Color::Red, Color::Black);
                println!("Unknown command: {}", command);
                vga_buffer::set_color(Color::White, Color::Black);
            }
        }
    }
}

pub async fn kernel_shell() {
    let mut shell = Shell::new();
    shell.run().await;
}
