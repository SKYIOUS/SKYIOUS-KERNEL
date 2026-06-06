use crate::event::Event;
use crate::drawing::Backbuffer;
use crate::color::Color;
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;

pub trait App {
    fn title(&self) -> &str;
    fn handle_event(&mut self, event: Event, content_x: usize, content_y: usize, content_w: usize, content_h: usize);
    fn render(&self, bb: &mut Backbuffer, cx: usize, cy: usize, cw: usize, ch: usize);
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any;
}

pub struct SkyTerm {
    pub lines: Vec<String>,
    pub cursor_blink: bool,
    pub input: String,
    prompt: String,
}

impl SkyTerm {
    pub fn new() -> Self {
        SkyTerm {
            lines: vec![
                String::from("SkyOS Terminal v0.1"),
                String::from("Type commands below."),
                String::from(""),
            ],
            cursor_blink: true,
            input: String::new(),
            prompt: String::from("$ "),
        }
    }
}

impl App for SkyTerm {
    fn title(&self) -> &str { "Terminal" }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any { self }

    fn handle_event(&mut self, event: Event, _cx: usize, _cy: usize, _cw: usize, _ch: usize) {
        match event {
            Event::KeyDown(code) => {
                match code {
                    // Letters a-z
                    16..=25 => {
                        let c = (b'a' + (code - 16) as u8) as char;
                        self.input.push(c);
                    }
                    // Numbers 0-9 (top row)
                    2..=11 => {
                        let n = if code == 2 { '1' } else if code == 3 { '2' } else if code == 4 { '3' }
                            else if code == 5 { '4' } else if code == 6 { '5' } else if code == 7 { '6' }
                            else if code == 8 { '7' } else if code == 9 { '8' } else if code == 10 { '9' }
                            else { '0' };
                        self.input.push(n);
                    }
                    57 => { self.input.push(' '); }
                    28 => {
                        let cmd = self.input.clone();
                        self.lines.push(alloc::format!("$ {}", cmd));
                        self.input.clear();
                        self.lines.push(String::from("  (command executed)"));
                    }
                    14 => { self.input.pop(); }
                    _ => {}
                }
            }
            _ => {}
        }
        if self.lines.len() > 100 {
            self.lines.drain(0..50);
        }
    }

    fn render(&self, bb: &mut Backbuffer, cx: usize, cy: usize, cw: usize, ch: usize) {
        bb.fill_rect(cx, cy, cw, ch, Color::BLACK);
        bb.draw_rect(cx, cy, cw, ch, Color::GREEN);
        let mut line_y = cy + 4;
        for line in &self.lines {
            if line_y + 8 > cy + ch - 14 { break; }
            bb.draw_text(cx + 4, line_y, line, Color::GREEN, Color::BLACK);
            line_y += 10;
        }
        let prompt_line = alloc::format!("{}{}", self.prompt, self.input);
        if line_y + 8 <= cy + ch - 4 {
            bb.draw_text(cx + 4, line_y, &prompt_line, Color::GREEN, Color::BLACK);
            if self.cursor_blink {
                let cursor_x = cx + 4 + prompt_line.len() * 8;
                bb.fill_rect(cursor_x, line_y, 8, 10, Color::GREEN);
            }
        }
    }
}

pub struct SkyCalc {
    pub display: String,
    pub first: i64,
    pub second: Option<i64>,
    pub op: Option<char>,
    pub fresh: bool,
}

impl SkyCalc {
    pub fn new() -> Self {
        SkyCalc {
            display: String::from("0"),
            first: 0,
            second: None,
            op: None,
            fresh: true,
        }
    }
}

impl App for SkyCalc {
    fn title(&self) -> &str { "Calculator" }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any { self }

    fn handle_event(&mut self, event: Event, _cx: usize, _cy: usize, _cw: usize, _ch: usize) {
        match event {
            Event::KeyDown(code) => {
                let ch = match code {
                    16 => 'q', 17 => 'w', 18 => 'e', 19 => 'r', 20 => 't', 21 => 'y',
                    24 => 'o', 25 => 'p',
                    18 => 'e',
                    2 => '1', 3 => '2', 4 => '3', 5 => '4', 6 => '5',
                    7 => '6', 8 => '7', 9 => '8', 10 => '9', 11 => '0',
                    13 => '=', 28 => '\n', 14 => '\x7f',
                    51 => ',', 52 => '.',
                    _ => '\0',
                };
                if ch == '\0' { return; }
                if ch == '\x7f' {
                    self.display.pop();
                    if self.display.is_empty() { self.display = String::from("0"); }
                    return;
                }
                if ch == '\n' || ch == '=' {
                    if let Some(op) = self.op {
                        let second = self.display.parse::<i64>().unwrap_or(0);
                        let result = match op {
                            '+' => self.first + second,
                            '-' => self.first - second,
                            '*' => self.first * second,
                            '/' => if second != 0 { self.first / second } else { 0 },
                            _ => 0,
                        };
                        self.display = alloc::format!("{}", result);
                        self.first = result;
                        self.op = None;
                    }
                    return;
                }
                if ch == '+' || ch == '-' || ch == '*' || ch == '/' {
                    self.first = self.display.parse::<i64>().unwrap_or(0);
                    self.op = Some(ch);
                    self.display = String::from("0");
                    return;
                }
                if ch.is_digit(10) || ch == '.' {
                    if self.display == "0" { self.display.clear(); }
                    self.display.push(ch);
                }
            }
            _ => {}
        }
    }

    fn render(&self, bb: &mut Backbuffer, cx: usize, cy: usize, cw: usize, ch: usize) {
        bb.fill_rect(cx, cy, cw, ch, Color::rgb(60, 60, 70));
        bb.draw_rect(cx, cy, cw, ch, Color::LIGHT_GRAY);
        let disp_y = cy + 8;
        bb.fill_rect(cx + 8, disp_y, cw - 16, 30, Color::rgb(30, 30, 40));
        bb.draw_rect(cx + 8, disp_y, cw - 16, 30, Color::WHITE);
        bb.draw_text(cx + 12, disp_y + 10, &self.display, Color::WHITE, Color::rgb(30, 30, 40));
        let btn_labels = [
            "7","8","9","/",
            "4","5","6","*",
            "1","2","3","-",
            "0",".","=","+",
        ];
        let btn_w = 48;
        let btn_h = 36;
        let gap = 4;
        let start_x = cx + (cw - (4 * btn_w + 3 * gap)) / 2;
        let start_y = disp_y + 40;
        for (i, &label) in btn_labels.iter().enumerate() {
            let col = i % 4;
            let row = i / 4;
            let bx = start_x + col * (btn_w + gap);
            let by = start_y + row * (btn_h + gap);
            let bg = if label == "=" { Color::ORANGE } else { Color::rgb(50, 50, 60) };
            bb.fill_rect(bx, by, btn_w, btn_h, bg);
            bb.draw_rect(bx, by, btn_w, btn_h, Color::LIGHT_GRAY);
            let text_x = bx + (btn_w - 8) / 2;
            let text_y = by + (btn_h - 8) / 2;
            bb.draw_text(text_x, text_y, label, Color::WHITE, bg);
        }
    }
}
