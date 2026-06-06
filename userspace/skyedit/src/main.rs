#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::ffi::CString;
use core::ffi::CStr;
use skyos_libc::syscall::{write, exit, open, close, read};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { exit(1); }

fn eprint(s: &str) { let _ = write(2, s.as_bytes()); }

fn read_file(path: &str) -> Option<Vec<u8>> {
    let c = CString::new(path.as_bytes()).ok()?;
    let fd = open(c.as_ptr() as *const u8, 0);
    if fd >= 0xFFFF_FFFF_FFFF_FF00 { return None; }
    let mut data = vec![];
    let mut buf = [0u8; 4096];
    loop {
        let n = read(fd, &mut buf);
        if n >= 0xFFFF_FFFF_FFFF_FF00 || n == 0 { break; }
        data.extend_from_slice(&buf[..n as usize]);
    }
    close(fd);
    Some(data)
}

fn write_file(path: &str, data: &[u8]) -> bool {
    let c = CString::new(path.as_bytes()).ok().unwrap();
    let fd = open(c.as_ptr() as *const u8, 0x0201 | 0x0040);
    if fd >= 0xFFFF_FFFF_FFFF_FF00 { return false; }
    write(fd, data);
    close(fd);
    true
}

fn stdin_read_ch() -> u8 {
    let mut ch = [0u8; 1];
    let n = skyos_libc::syscall::syscall3(skyos_libc::SYS_READ, 0, ch.as_mut_ptr() as u64, 1);
    if n != 1 { 4 } else { ch[0] }
}

fn is_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "break"
        | "continue" | "return" | "goto" | "static" | "extern" | "const" | "volatile"
        | "struct" | "union" | "enum" | "typedef" | "sizeof" | "int" | "void" | "char"
        | "long" | "short" | "unsigned" | "signed" | "float" | "double" | "bool"
        | "true" | "false" | "NULL" | "fn" | "let" | "mut" | "pub" | "use" | "mod"
        | "impl" | "trait" | "match" | "self" | "super" | "crate" | "where"
        | "as" | "in" | "ifdef" | "ifndef" | "define" | "include" | "pragma" | "endif")
}

fn highlight_line(line: &str) -> String {
    let mut out = String::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            out.push_str("\x1b[2m");
            out.push_str(&line[i..]);
            out.push_str("\x1b[0m");
            break;
        }
        if bytes[i] == b'#' {
            out.push_str("\x1b[35m");
            out.push_str(&line[i..]);
            out.push_str("\x1b[0m");
            break;
        }
        if bytes[i] == b'"' {
            out.push_str("\x1b[32m");
            out.push('"');
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() { out.push(bytes[i] as char); i += 1; }
                out.push(bytes[i] as char);
                i += 1;
            }
            if i < bytes.len() { out.push('"'); i += 1; }
            out.push_str("\x1b[0m");
            continue;
        }
        if bytes[i] == b'\'' {
            out.push_str("\x1b[32m");
            out.push('\'');
            i += 1;
            while i < bytes.len() && bytes[i] != b'\'' { out.push(bytes[i] as char); i += 1; }
            if i < bytes.len() { out.push('\''); i += 1; }
            out.push_str("\x1b[0m");
            continue;
        }
        if bytes[i] >= b'0' && bytes[i] <= b'9' {
            out.push_str("\x1b[33m");
            while i < bytes.len() && bytes[i] >= b'0' && bytes[i] <= b'9' { out.push(bytes[i] as char); i += 1; }
            out.push_str("\x1b[0m");
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push_str("\x1b[2m");
            out.push_str("/*");
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' { out.push_str("*/"); i += 2; break; }
                out.push(bytes[i] as char);
                i += 1;
            }
            out.push_str("\x1b[0m");
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            let word = &line[start..i];
            if is_keyword(word) { out.push_str("\x1b[36m"); }
            else { out.push_str("\x1b[0m"); }
            out.push_str(word);
            out.push_str("\x1b[0m");
            continue;
        }
        if bytes[i] == b'{' || bytes[i] == b'}' || bytes[i] == b'(' || bytes[i] == b')' {
            out.push_str("\x1b[91m");
            out.push(bytes[i] as char);
            out.push_str("\x1b[0m");
            i += 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn display(lines: &[String], cursor_line: usize, cursor_col: usize, top_line: usize, rows: usize, mode: &str, modified: bool, cmd_buf: &str, lang: &str) {
    write(1, b"\x1b[2J\x1b[H");
    let m = if mode == "INSERT" { "INSERT" } else if mode == "CMD" { "CMD" } else { "NORMAL" };
    let status = alloc::format!("SkyEdit {}  line {}/{}  col {}  {}  {}",
        m, cursor_line + 1, lines.len(), cursor_col + 1, lang,
        if modified { "[+]" } else { "" });
    write(1, b"\x1b[7m");
    let max_w = 79;
    if status.len() > max_w { write(1, status[..max_w].as_bytes()); }
    else { write(1, status.as_bytes()); for _ in status.len()..max_w { write(1, b" "); } }
    write(1, b"\x1b[0m\n");

    let end = core::cmp::min(top_line + rows - 1, lines.len());
    for i in top_line..end {
        if mode == "CMD" { write(1, lines[i].as_bytes()); }
        else { write(1, highlight_line(&lines[i]).as_bytes()); }
        let _ = write(1, b"\x1b[K");
        if i < end - 1 { write(1, b"\n"); }
    }
    let blank = top_line + rows - 1;
    if end < blank { for _ in 0..blank - end { write(1, b"\n\x1b[K"); } }

    if mode == "CMD" {
        let prompt = alloc::format!("\x1b[{};1H:{}", rows + 1, cmd_buf);
        write(1, prompt.as_bytes());
        write(1, b" \x1b[K");
        let col = 2 + cmd_buf.len();
        let pos = alloc::format!("\x1b[{};{}H", rows + 1, col);
        write(1, pos.as_bytes());
    } else {
        let line_off = cursor_line - top_line;
        let row = line_off + 2;
        let col = cursor_col + 1;
        let pos = alloc::format!("\x1b[{};{}H", row, col);
        write(1, pos.as_bytes());
    }
}

fn push_undo(undo_stack: &mut Vec<Vec<String>>, lines: &[String]) {
    if undo_stack.len() > 100 { undo_stack.remove(0); }
    undo_stack.push(lines.to_vec());
}

fn editor_loop(lines: &mut Vec<String>, path: &str) {
    let mut cursor_line = 0usize;
    let mut cursor_col = 0usize;
    let mut top_line = 0usize;
    let rows = 24;
    let mut modified = false;
    let mut mode = "NORMAL";
    let mut cmd_buf = String::new();
    let mut yank_line: Option<String> = None;
    let mut undo_stack: Vec<Vec<String>> = Vec::new();
    let mut last_key = 0u8;

    let lang = if path.ends_with(".rs") { "Rust" }
        else if path.ends_with(".c") || path.ends_with(".h") { "C" }
        else if path.ends_with(".s") || path.ends_with(".S") || path.ends_with(".asm") { "Asm" }
        else { "Text" };

    loop {
        if lines.is_empty() { lines.push(String::new()); }
        if cursor_line >= lines.len() { cursor_line = lines.len().saturating_sub(1); }
        if cursor_col > lines[cursor_line].len() { cursor_col = lines[cursor_line].len(); }

        display(lines, cursor_line, cursor_col, top_line, rows, mode, modified, &cmd_buf, lang);

        let ch = stdin_read_ch();

        if mode == "CMD" {
            match ch {
                b'\n' | b'\r' => {
                    let trimmed = cmd_buf.trim();
                    if trimmed == "q" || trimmed == "q!" || trimmed == "quit" {
                        if trimmed == "q!" || !modified { write(1, b"\n"); break; }
                        else { cmd_buf.clear(); mode = "NORMAL"; }
                    } else if trimmed == "w" || trimmed == "w!" {
                        write_file(path, lines.join("\n").as_bytes());
                        modified = false;
                        cmd_buf.clear(); mode = "NORMAL";
                    } else if trimmed == "wq" || trimmed == "wq!" {
                        write_file(path, lines.join("\n").as_bytes());
                        write(1, b"\n"); break;
                    } else {
                        cmd_buf.clear(); mode = "NORMAL";
                    }
                }
                0x1b => { cmd_buf.clear(); mode = "NORMAL"; }
                0x7f | 8 => { cmd_buf.pop(); }
                c if c >= 32 && c <= 126 => {
                    if cmd_buf.len() < 20 { cmd_buf.push(c as char); }
                }
                _ => {}
            }
            continue;
        }

        if mode == "INSERT" {
            match ch {
                0x1b => { mode = "NORMAL"; }
                b'\n' | b'\r' => {
                    push_undo(&mut undo_stack, lines);
                    let rest = lines[cursor_line].split_off(cursor_col);
                    lines.insert(cursor_line + 1, rest);
                    cursor_line += 1; cursor_col = 0;
                    modified = true;
                }
                0x7f | 8 => {
                    push_undo(&mut undo_stack, lines);
                    if cursor_col > 0 {
                        lines[cursor_line].remove(cursor_col - 1);
                        cursor_col -= 1; modified = true;
                    } else if cursor_line > 0 {
                        let prev_len = lines[cursor_line - 1].len();
                        let cur = lines.remove(cursor_line);
                        lines[cursor_line - 1].push_str(&cur);
                        cursor_line -= 1; cursor_col = prev_len;
                        modified = true;
                    }
                }
                3 | 4 => { write(1, b"\n"); break; }
                c if c >= 32 && c <= 126 => {
                    push_undo(&mut undo_stack, lines);
                    lines[cursor_line].insert(cursor_col, c as char);
                    cursor_col += 1; modified = true;
                }
                _ => {}
            }
            continue;
        }

        match ch {
            0x1b => {
                let ch2 = stdin_read_ch();
                if ch2 == b'[' {
                    let ch3 = stdin_read_ch();
                    match ch3 {
                        b'A' => { if cursor_line > 0 { cursor_line -= 1; }
                            if top_line > cursor_line { top_line = cursor_line; } }
                        b'B' => { if cursor_line + 1 < lines.len() { cursor_line += 1; }
                            if cursor_line >= top_line + rows - 1 { top_line = cursor_line - rows + 2; } }
                        b'C' => { if cursor_col < lines[cursor_line].len() { cursor_col += 1; } }
                        b'D' => { if cursor_col > 0 { cursor_col -= 1; } }
                        _ => {}
                    }
                    if cursor_col > lines[cursor_line].len() { cursor_col = lines[cursor_line].len(); }
                }
            }
            b':' => { mode = "CMD"; cmd_buf.clear(); }
            b'i' => { mode = "INSERT"; }
            b'a' => {
                mode = "INSERT";
                if cursor_col < lines[cursor_line].len() { cursor_col += 1; }
            }
            b'I' => { cursor_col = 0; mode = "INSERT"; }
            b'A' => { cursor_col = lines[cursor_line].len(); mode = "INSERT"; }
            b'o' => {
                push_undo(&mut undo_stack, lines);
                lines.insert(cursor_line + 1, String::new());
                cursor_line += 1; cursor_col = 0;
                modified = true; mode = "INSERT";
            }
            b'O' => {
                push_undo(&mut undo_stack, lines);
                lines.insert(cursor_line, String::new());
                cursor_col = 0;
                modified = true; mode = "INSERT";
            }
            b'h' => { if cursor_col > 0 { cursor_col -= 1; } }
            b'l' => { if cursor_col < lines[cursor_line].len() { cursor_col += 1; } }
            b'j' => {
                if cursor_line + 1 < lines.len() { cursor_line += 1; }
                if cursor_line >= top_line + rows - 1 { top_line = cursor_line - rows + 2; }
            }
            b'k' => {
                if cursor_line > 0 { cursor_line -= 1; }
                if top_line > cursor_line { top_line = cursor_line; }
            }
            b'0' => { cursor_col = 0; }
            b'$' => { cursor_col = lines[cursor_line].len(); }
            b'x' => {
                if !lines[cursor_line].is_empty() && cursor_col < lines[cursor_line].len() {
                    push_undo(&mut undo_stack, lines);
                    lines[cursor_line].remove(cursor_col);
                    modified = true;
                }
            }
            b'X' => {
                if cursor_col > 0 {
                    push_undo(&mut undo_stack, lines);
                    lines[cursor_line].remove(cursor_col - 1);
                    cursor_col -= 1; modified = true;
                }
            }
            b'd' if last_key == b'd' => {
                push_undo(&mut undo_stack, lines);
                if lines.len() > 1 { lines.remove(cursor_line);
                    if cursor_line >= lines.len() { cursor_line = lines.len().saturating_sub(1); }
                } else { lines[0].clear(); cursor_col = 0; }
                modified = true; last_key = 0;
            }
            b'y' if last_key == b'y' => {
                yank_line = Some(lines[cursor_line].clone());
                last_key = 0;
            }
            b'p' => {
                if let Some(ref yanked) = yank_line {
                    push_undo(&mut undo_stack, lines);
                    lines.insert(cursor_line + 1, yanked.clone());
                    modified = true;
                }
            }
            b'P' => {
                if let Some(ref yanked) = yank_line {
                    push_undo(&mut undo_stack, lines);
                    lines.insert(cursor_line, yanked.clone());
                    modified = true;
                }
            }
            b'u' => {
                if let Some(prev) = undo_stack.pop() {
                    *lines = prev;
                    if cursor_line >= lines.len() { cursor_line = lines.len().saturating_sub(1); }
                    if cursor_col > lines[cursor_line].len() { cursor_col = lines[cursor_line].len(); }
                }
            }
            b'G' => {
                cursor_line = lines.len() - 1;
                if cursor_line >= top_line + rows - 1 { top_line = if cursor_line > rows - 1 { cursor_line - rows + 2 } else { 0 }; }
            }
            b'g' if last_key != b'g' => { last_key = b'g'; continue; }
            b'g' if last_key == b'g' => {
                cursor_line = 0; top_line = 0; last_key = 0;
            }
            6 => { // Ctrl+F
                if top_line + rows < lines.len() { top_line += rows; cursor_line = top_line; }
            }
            2 => { // Ctrl+B
                if top_line >= rows { top_line -= rows; cursor_line = top_line; } else { top_line = 0; cursor_line = 0; }
            }
            19 => { // Ctrl+S
                write_file(path, lines.join("\n").as_bytes());
                modified = false;
            }
            17 => { // Ctrl+Q
                if modified {
                    eprint("\nSave? (y/n): ");
                    let a = stdin_read_ch();
                    if a == b'y' || a == b'Y' { write_file(path, lines.join("\n").as_bytes()); }
                }
                write(1, b"\n"); break;
            }
            3 | 4 => { write(1, b"\n"); break; }
            _ => {}
        }
        last_key = if ch != b'g' && ch != b'd' && ch != b'y' { 0 } else if last_key == ch { 0 } else { last_key };
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let path = if _argc > 1 {
        unsafe {
            let ptr = *_argv.add(1);
            if ptr.is_null() { "/tmp/untitled" } else {
                CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("/tmp/untitled")
            }
        }
    } else {
        eprint("Usage: skyedit <file>\n");
        return 1;
    };

    let data = read_file(path).unwrap_or_default();
    let content = core::str::from_utf8(&data).unwrap_or("");
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    if lines.is_empty() { lines.push(String::new()); }

    editor_loop(&mut lines, path);
    0
}
