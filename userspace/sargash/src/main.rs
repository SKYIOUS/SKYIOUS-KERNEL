#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::ffi::CString;
use skyos_libc::syscall::{write, exit, fork, execve, wait4, open, close, pipe, dup2, chdir, getcwd};

const PATH_DIRS: &[&str] = &["/bin", "/sbin", "/usr/bin", "/usr/local/bin"];
const MAX_LINE: usize = 4096;
const PROMPT: &[u8] = b"skyos$ ";
const HISTFILE: &str = "/home/root/.sargash_history\0";
const HIST_MAX: usize = 500;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
}

static mut ENV: BTreeMap<String, String> = BTreeMap::new();
static mut HISTORY: Vec<String> = Vec::new();
static mut HIST_POS: isize = -1;
static mut BG_JOBS: BTreeMap<u32, String> = BTreeMap::new();
static mut NEXT_JOB: u32 = 1;
static mut LAST_EXIT: i32 = 0;

fn env_init() {
    unsafe {
        ENV.insert("PATH".into(), "/bin:/sbin".into());
        ENV.insert("HOME".into(), "/home/root".into());
        ENV.insert("TERM".into(), "skycon".into());
        ENV.insert("SHELL".into(), "/bin/sargash".into());
    }
}

fn env_get(key: &str) -> Option<String> {
    unsafe { ENV.get(key).cloned() }
}

fn env_set(key: &str, val: &str) {
    unsafe { ENV.insert(key.to_string(), val.to_string()); }
}

fn env_iter() -> Vec<(String, String)> {
    unsafe { ENV.iter().map(|(k, v)| (k.clone(), v.clone())).collect() }
}

fn expand_vars(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i+1] == b'{' {
                let end = s[i+2..].find('}').map(|p| i + 2 + p).unwrap_or(bytes.len());
                let var = &s[i+2..end];
                let val = env_get(var).unwrap_or_default();
                out.push_str(&val);
                i = end + 1;
                continue;
            }
            if bytes[i+1] == b'?' {
                out.push_str(&alloc::format!("{}", unsafe { LAST_EXIT }));
                i += 2;
                continue;
            }
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') { end += 1; }
            if end > start {
                let var = &s[start..end];
                let val = env_get(var).unwrap_or_default();
                out.push_str(&val);
                i = end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn build_envp() -> Vec<u64> {
    let mut v = vec![];
    for (k, val) in env_iter() {
        let s = alloc::format!("{}={}", k, val);
        if let Ok(c) = CString::new(s.as_bytes()) {
            v.push(c.into_raw() as u64);
        }
    }
    v.push(0);
    v
}

fn hist_load() {
    unsafe {
        let c = CString::new(HISTFILE.as_bytes()).unwrap();
        let fd = open(c.as_ptr() as *const u8, 0);
        if fd >= 0xFFFF_FFFF_FFFF_FF00 { return; }
        let mut buf = [0u8; 4096];
        let mut data = Vec::new();
        loop {
            let n = skyos_libc::syscall::read(fd, &mut buf);
            if (n as i64) <= 0 { break; }
            data.extend_from_slice(&buf[..n as usize]);
        }
        close(fd);
        if data.is_empty() { return; }
        let s = core::str::from_utf8(&data).unwrap_or("");
        let mut count = 0;
        for line in s.lines() {
            if count >= HIST_MAX { break; }
            HISTORY.push(line.into());
            count += 1;
        }
        HIST_POS = HISTORY.len() as isize;
    }
}

fn hist_save() {
    unsafe {
        let c = CString::new(HISTFILE.as_bytes()).unwrap();
        let fd = open(c.as_ptr() as *const u8, 0x0201 | 0x0040);
        if fd >= 0xFFFF_FFFF_FFFF_FF00 { return; }
        let start = if HISTORY.len() > HIST_MAX { HISTORY.len() - HIST_MAX } else { 0 };
        for i in start..HISTORY.len() {
            let s = alloc::format!("{}\n", HISTORY[i]);
            skyos_libc::syscall::write(fd, s.as_bytes());
        }
        close(fd);
    }
}

fn hist_add(line: &str) {
    unsafe {
        if line.is_empty() { return; }
        if HISTORY.last().map(|l| l.as_str() == line).unwrap_or(false) { return; }
        HISTORY.push(line.into());
        if HISTORY.len() > HIST_MAX * 2 { HISTORY.remove(0); }
        HIST_POS = HISTORY.len() as isize;
    }
}

fn hist_prev() -> Option<String> {
    unsafe {
        if HISTORY.is_empty() { return None; }
        let new_pos = HIST_POS - 1;
        if new_pos < 0 { return None; }
        HIST_POS = new_pos;
        Some(HISTORY[HIST_POS as usize].clone())
    }
}

fn hist_next() -> Option<String> {
    unsafe {
        if HISTORY.is_empty() { return None; }
        let new_pos = HIST_POS + 1;
        if new_pos as usize >= HISTORY.len() {
            HIST_POS = HISTORY.len() as isize;
            return Some(String::new());
        }
        HIST_POS = new_pos;
        Some(HISTORY[HIST_POS as usize].clone())
    }
}

fn repaint(buf: &[u8], pos: usize, len: usize) {
    let _ = write(1, b"\r\x1b[K");
    let _ = write(1, &buf[..len]);
    let move_left = len - pos;
    if move_left > 0 {
        let esc = alloc::format!("\x1b[{}D", move_left);
        let _ = write(1, esc.as_bytes());
    }
}

fn tab_complete(buf: &mut [u8], pos: &mut usize, len: &mut usize) {
    if *len == 0 { return; }
    let mut word_start = *pos;
    while word_start > 0 && buf[word_start - 1] != b' ' { word_start -= 1; }
    let prefix = core::str::from_utf8(&buf[word_start..*pos]).unwrap_or("");
    if prefix.is_empty() { return; }

    let mut matches = Vec::new();
    if let Some(paths) = env_get("PATH") {
        for dir in paths.split(':') {
            let dirc = CString::new(dir.as_bytes()).unwrap();
            let fd = open(dirc.as_ptr() as *const u8, 0);
            if fd >= 0xFFFF_FFFF_FFFF_FF00 { continue; }
            let mut dent = [0u8; 280];
            loop {
                let n = skyos_libc::syscall::getdents64(fd, dent.as_mut_ptr(), dent.len());
                if (n as i64) <= 0 { break; }
                let mut off = 0;
                while off < n as usize {
                    if off + 19 > dent.len() { break; }
                    let reclen = u16::from_le_bytes([dent[off+16], dent[off+17]]) as usize;
                    if reclen < 19 || off + reclen > dent.len() { break; }
                    let name_start = off + 19;
                    let name_bytes = &dent[name_start..off + reclen];
                    let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
                    let name = core::str::from_utf8(&name_bytes[..name_end]).unwrap_or("");
                    if name.starts_with(prefix) && name != "." && name != ".." {
                        matches.push(name.to_string());
                    }
                    off += reclen;
                }
            }
            close(fd);
        }
    }

    if matches.is_empty() { return; }
    matches.sort();
    matches.dedup();

    if matches.len() == 1 {
        let rest = &matches[0][prefix.len()..];
        let rest_bytes = rest.as_bytes();
        let space_left = buf.len() - 1 - *len;
        let to_copy = core::cmp::min(rest_bytes.len(), space_left);
        buf[*pos..*pos + to_copy].copy_from_slice(&rest_bytes[..to_copy]);
        *len += to_copy;
        *pos += to_copy;
        buf[*len] = b' ';
        *len += 1;
        *pos += 1;
        repaint(buf, *pos, *len);
    } else {
        let _ = write(1, b"\r\n");
        for m in &matches {
            let _ = write(1, m.as_bytes());
            let _ = write(1, b"  ");
        }
        let _ = write(1, b"\r\n");
        let _ = write(1, PROMPT);
        repaint(buf, *pos, *len);
    }
}

fn stdin_read(buf: &mut [u8]) -> usize {
    let mut pos: usize = 0;
    let mut len: usize = 0;
    loop {
        let mut ch = [0u8; 1];
        let n = skyos_libc::syscall::syscall3(skyos_libc::SYS_READ, 0, ch.as_mut_ptr() as u64, 1);
        if n != 1 { break; }
        let c = ch[0];

        if c == b'\n' || c == b'\r' {
            buf[len] = 0;
            let _ = write(1, b"\n");
            hist_add(core::str::from_utf8(&buf[..len]).unwrap_or(""));
            return len;
        }

        if c == 3 {
            return usize::MAX;
        }

        if c == b'\t' {
            tab_complete(buf, &mut pos, &mut len);
            continue;
        }

        if c == 0x1b {
            let mut seq = [0u8; 2];
            let n1 = skyos_libc::syscall::syscall3(skyos_libc::SYS_READ, 0, seq.as_mut_ptr() as u64, 1);
            if n1 != 1 { continue; }
            if seq[0] == b'[' {
                let mut cmd = [0u8; 1];
                let n2 = skyos_libc::syscall::syscall3(skyos_libc::SYS_READ, 0, cmd.as_mut_ptr() as u64, 1);
                if n2 != 1 { continue; }
                match cmd[0] {
                    b'A' => {
                        if let Some(entry) = hist_prev() {
                            len = core::cmp::min(entry.len(), buf.len() - 1);
                            buf[..len].copy_from_slice(entry.as_bytes());
                            pos = len;
                            repaint(buf, pos, len);
                        }
                    }
                    b'B' => {
                        if let Some(entry) = hist_next() {
                            len = core::cmp::min(entry.len(), buf.len() - 1);
                            buf[..len].copy_from_slice(entry.as_bytes());
                            pos = len;
                            repaint(buf, pos, len);
                        }
                    }
                    b'C' => {
                        if pos < len { pos += 1; repaint(buf, pos, len); }
                    }
                    b'D' => {
                        if pos > 0 { pos -= 1; repaint(buf, pos, len); }
                    }
                    b'H' => {
                        pos = 0; repaint(buf, pos, len);
                    }
                    b'F' => {
                        pos = len; repaint(buf, pos, len);
                    }
                    b'3' => {
                        let mut tilde = [0u8; 1];
                        let n3 = skyos_libc::syscall::syscall3(skyos_libc::SYS_READ, 0, tilde.as_mut_ptr() as u64, 1);
                        if n3 == 1 && tilde[0] == b'~' && pos < len {
                            for j in pos..len { buf[j] = buf[j+1]; }
                            len -= 1;
                            repaint(buf, pos, len);
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        if c == 0x7f || c == 8 {
            if pos > 0 {
                for j in pos - 1..len { buf[j] = buf[j+1]; }
                len -= 1;
                pos -= 1;
                repaint(buf, pos, len);
            }
            continue;
        }

        if c == 4 {
            if len == 0 { return usize::MAX; }
            if pos < len {
                for j in pos..len { buf[j] = buf[j+1]; }
                len -= 1;
                repaint(buf, pos, len);
            }
            continue;
        }

        if len < buf.len() - 1 {
            if pos < len {
                for j in (pos..len).rev() { buf[j+1] = buf[j]; }
            }
            buf[pos] = c;
            pos += 1;
            len += 1;
            repaint(buf, pos, len);
        }
    }
    buf[len] = 0;
    hist_add(core::str::from_utf8(&buf[..len]).unwrap_or(""));
    len
}

#[derive(Clone)]
struct Redir {
    fd: usize,
    path: String,
    append: bool,
}

#[derive(Clone)]
struct Command {
    argv_strs: Vec<String>,
    redir_in: Option<Redir>,
    redir_out: Option<Redir>,
    background: bool,
}

fn tokenize_pipe(seg: &str) -> Vec<Vec<String>> {
    let mut commands: Vec<Vec<String>> = vec![];
    let mut current: Vec<String> = vec![];
    let mut i = 0;
    let chars: Vec<char> = seg.chars().collect();
    while i < chars.len() {
        if chars[i] == '#' { break; }
        if chars[i] == '|' {
            if !current.is_empty() { commands.push(core::mem::take(&mut current)); }
            i += 1;
            continue;
        }
        if chars[i] == ' ' || chars[i] == '\t' { i += 1; continue; }
        let mut tok = String::new();
        while i < chars.len() {
            let c = chars[i];
            if c == '\'' {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    tok.push(chars[i]); i += 1;
                }
                if i < chars.len() { i += 1; }
                continue;
            }
            if c == '"' {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i+1 < chars.len() { i += 1; }
                    tok.push(chars[i]); i += 1;
                }
                if i < chars.len() { i += 1; }
                continue;
            }
            if c == '\\' && i+1 < chars.len() {
                i += 1; tok.push(chars[i]); i += 1;
                continue;
            }
            if c == ' ' || c == '\t' || c == '|' { break; }
            if c == '&' { tok.push(c); i += 1; break; }
            tok.push(c); i += 1;
        }
        if !tok.is_empty() { current.push(tok); }
    }
    if !current.is_empty() { commands.push(current); }
    commands
}

fn parse_redirects(tokens: &mut Vec<String>) -> (Option<Redir>, Option<Redir>) {
    let mut redir_in = None;
    let mut redir_out = None;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].clone();
        if t == "<" && i+1 < tokens.len() {
            redir_in = Some(Redir { fd: 0, path: tokens[i+1].clone(), append: false });
            tokens.remove(i); tokens.remove(i);
            continue;
        }
        if t == ">" && i+1 < tokens.len() {
            redir_out = Some(Redir { fd: 1, path: tokens[i+1].clone(), append: false });
            tokens.remove(i); tokens.remove(i);
            continue;
        }
        if t == ">>" && i+1 < tokens.len() {
            redir_out = Some(Redir { fd: 1, path: tokens[i+1].clone(), append: true });
            tokens.remove(i); tokens.remove(i);
            continue;
        }
        if t == "2>" && i+1 < tokens.len() {
            redir_out = Some(Redir { fd: 2, path: tokens[i+1].clone(), append: false });
            tokens.remove(i); tokens.remove(i);
            continue;
        }
        i += 1;
    }
    (redir_in, redir_out)
}

fn try_resolve(cmd: &str) -> Option<String> {
    if cmd.contains('/') {
        return Some(cmd.to_string());
    }
    if let Some(paths) = env_get("PATH") {
        for dir in paths.split(':') {
            let full = alloc::format!("{}/{}", dir, cmd);
            let c = CString::new(full.as_bytes()).ok()?;
            let fd = open(c.as_ptr() as *const u8, 0);
            if fd < 0xFFFF_FFFF_FFFF_FF00 {
                close(fd);
                return Some(full);
            }
        }
    }
    for dir in PATH_DIRS {
        let full = alloc::format!("{}/{}", dir, cmd);
        let c = CString::new(full.as_bytes()).ok()?;
        let fd = open(c.as_ptr() as *const u8, 0);
        if fd < 0xFFFF_FFFF_FFFF_FF00 {
            close(fd);
            return Some(full);
        }
    }
    None
}

fn do_exec(cmd: &Command) {
    if cmd.argv_strs.is_empty() { return; }

    let path_str = &cmd.argv_strs[0];
    let resolved = try_resolve(path_str);
    let resolved_path = resolved.as_deref().unwrap_or(path_str);

    let path_c = CString::new(resolved_path.as_bytes()).unwrap();
    let mut new_argv: Vec<u64> = vec![path_c.as_ptr() as u64];
    for a in cmd.argv_strs.iter().skip(1) {
        if let Ok(c) = CString::new(a.as_bytes()) {
            new_argv.push(c.into_raw() as u64);
        }
    }
    new_argv.push(0);

    let envp = build_envp();
    let _ = execve(path_c.as_ptr() as *const u8, new_argv.as_ptr() as *const *const u8, envp.as_ptr() as *const *const u8);
}

fn apply_redirects(cmd: &Command) {
    if let Some(ref r) = cmd.redir_in {
        let flags = 0;
        let c = CString::new(r.path.as_bytes()).unwrap();
        let fd = open(c.as_ptr() as *const u8, flags);
        if fd < 0xFFFF_FFFF_FFFF_FF00 {
            dup2(fd, r.fd as u64);
            close(fd);
        }
    }
    if let Some(ref r) = cmd.redir_out {
        let flags = if r.append { 0x0400 } else { 0x0201 | 0x0040 };
        let c = CString::new(r.path.as_bytes()).unwrap();
        let fd = open(c.as_ptr() as *const u8, flags);
        if fd < 0xFFFF_FFFF_FFFF_FF00 {
            dup2(fd, r.fd as u64);
            close(fd);
        }
    }
}

fn spawn_pipeline(cmds: &[Command]) {
    if cmds.is_empty() { return; }

    let back = cmds.iter().any(|c| c.background);

    if cmds.len() == 1 {
        let cmd = &cmds[0];
        if cmd.argv_strs.is_empty() { return; }
        let pid = fork();
        if pid == 0 {
            apply_redirects(cmd);
            do_exec(cmd);
            exit(1);
        }
        if cmd.background {
            unsafe {
                let job = NEXT_JOB;
                NEXT_JOB += 1;
                BG_JOBS.insert(job, alloc::format!("[{}] {}", pid, cmd.argv_strs[0]));
            }
        } else {
            let mut status: i32 = 0;
            wait4(pid as i64, &mut status, 0, core::ptr::null_mut());
        }
        return;
    }

    let n = cmds.len();
    let mut prev_read: i64 = -1;

    for (i, cmd) in cmds.iter().enumerate() {
        let mut p = [0i32; 2];
        let is_last = i == n - 1;

        if !is_last && pipe(p.as_mut_ptr() as *mut u32) >= 0xFFFF_FFFF_FFFF_FF00 {
            break;
        }

        let pid = fork();
        if pid == 0 {
            if !is_last {
                close(p[0] as u64);
                dup2(p[1] as u64, 1);
                close(p[1] as u64);
            }
            if prev_read >= 0 {
                dup2(prev_read as u64, 0);
                close(prev_read as u64);
            }
            apply_redirects(cmd);
            do_exec(cmd);
            exit(1);
        }

        if prev_read >= 0 { close(prev_read as u64); }
        if !is_last { close(p[1] as u64); prev_read = p[0] as i64; }
    }

    if back {
        unsafe {
            let job = NEXT_JOB;
            NEXT_JOB += 1;
            BG_JOBS.insert(job, alloc::format!("[{}] (pipeline)", job));
        }
    } else {
        let mut status: i32 = 0;
        wait4(-1, &mut status, 0, core::ptr::null_mut());
    }
}

fn do_cd(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() { return true; }
    let c = CString::new(trimmed.as_bytes()).unwrap();
    let ret = chdir(c.as_ptr() as *const u8);
    ret < 0xFFFF_FFFF_FFFF_FF00
}

fn do_pwd() {
    let mut buf = [0u8; 512];
    let ret = getcwd(buf.as_mut_ptr(), buf.len());
    if ret < 0xFFFF_FFFF_FFFF_FF00 {
        let len = ret as usize;
        if len > 0 && len <= buf.len() {
            write(1, &buf[..len]);
            write(1, b"\n");
        }
    }
}

fn do_jobs() {
    unsafe {
        for (job, desc) in &BG_JOBS {
            let s = alloc::format!("[{}] {}\n", job, desc);
            write(1, s.as_bytes());
        }
    }
}

fn do_fg(args: &[String]) -> bool {
    unsafe {
        if BG_JOBS.is_empty() {
            write(2, b"fg: no current job\n");
            return true;
        }
        let target = args.get(1).and_then(|s| s.parse::<u32>().ok());
        let pid = if let Some(job) = target {
            if let Some(desc) = BG_JOBS.get(&job) {
                let pid_str = desc.split(' ').nth(0).unwrap_or("");
                let pid = pid_str.trim_matches(&['[', ']', ' '][..]).parse::<i64>().ok();
                BG_JOBS.remove(&job);
                pid
            } else {
                write(2, b"fg: no such job\n"); return true;
            }
        } else {
            let last = BG_JOBS.iter().last();
            if let Some((job, desc)) = last {
                let pid_str = desc.split(' ').nth(0).unwrap_or("");
                let pid = pid_str.trim_matches(&['[', ']', ' '][..]).parse::<i64>().ok();
                BG_JOBS.remove(job);
                pid
            } else { None }
        };
        if let Some(pid) = pid {
            let mut status: i32 = 0;
            wait4(pid, &mut status, 0, core::ptr::null_mut());
        }
    }
    true
}

fn do_bg(args: &[String]) -> bool {
    unsafe {
        if BG_JOBS.is_empty() {
            write(2, b"bg: no current job\n");
            return true;
        }
        if let Some(job_str) = args.get(1) {
            if let Some(_) = BG_JOBS.get(&job_str.parse::<u32>().unwrap_or(0)) {
                write(1, b"bg: resumed\n");
            } else {
                write(2, b"bg: no such job\n");
            }
        } else {
            write(1, b"bg: resumed\n");
        }
    }
    true
}

fn builtin_cmd(argv: &[String]) -> bool {
    if argv.is_empty() { return true; }
    match argv[0].as_str() {
        "exit" => {
            hist_save();
            let code = argv.get(1).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
            exit(code);
        }
        "cd" => {
            let path = argv.get(1).map(|s| s.as_str()).unwrap_or("/");
            if !do_cd(path) {
                write(2, b"cd: failed\n");
            }
        }
        "pwd" => { do_pwd(); }
        "echo" => {
            for (i, arg) in argv.iter().enumerate().skip(1) {
                if i > 1 { write(1, b" "); }
                write(1, arg.as_bytes());
            }
            write(1, b"\n");
        }
        "export" => {
            for arg in argv.iter().skip(1) {
                if let Some(pos) = arg.find('=') {
                    let key = &arg[..pos];
                    let val = &arg[pos+1..];
                    env_set(key, val);
                }
            }
        }
        "help" => {
            write(1, b"SargaSH v0.3 - SkyOS Shell\n");
            write(1, b"Builtins: exit, cd, pwd, echo, export, help, jobs, fg, bg\n");
            write(1, b"Features: pipes, redirects, env, PATH, background(&), history, tab-complete, line-edit\n");
        }
        "jobs" => { do_jobs(); }
        "fg" => { do_fg(argv); }
        "bg" => { do_bg(argv); }
        _ => return false,
    }
    true
}

fn exec_segment(seg: &str) -> i32 {
    let expanded = expand_vars(seg);
    let seg = expanded.trim();
    if seg.is_empty() { return 0; }

    let cmds_in_pipe: Vec<Command> = tokenize_pipe(seg).into_iter().filter_map(|tokens| {
        if tokens.is_empty() { return None; }
        let mut toks = tokens;
        let background = toks.last().map(|s| s.as_str() == "&").unwrap_or(false);
        if background { toks.pop(); }
        let (redir_in, redir_out) = parse_redirects(&mut toks);
        Some(Command { argv_strs: toks, redir_in, redir_out, background })
    }).collect();

    if cmds_in_pipe.is_empty() { return 0; }

    if let Some(cmd) = cmds_in_pipe.first() {
        if builtin_cmd(&cmd.argv_strs) {
            return unsafe { LAST_EXIT };
        }
    }

    let back = cmds_in_pipe.iter().any(|c| c.background);
    if cmds_in_pipe.len() == 1 && !back {
        let cmd = &cmds_in_pipe[0];
        if cmd.argv_strs.is_empty() { return 0; }
        if cmd.argv_strs[0] == "test" || cmd.argv_strs[0] == "[" {
            return do_test(&cmd.argv_strs);
        }
    }

    if cmds_in_pipe.len() == 1 && !back && cmds_in_pipe[0].redir_in.is_none() && cmds_in_pipe[0].redir_out.is_none() {
        let pid = fork();
        if pid == 0 {
            apply_redirects(&cmds_in_pipe[0]);
            do_exec(&cmds_in_pipe[0]);
            exit(1);
        }
        let mut status: i32 = 0;
        wait4(pid as i64, &mut status, 0, core::ptr::null_mut());
        return status;
    }

    spawn_pipeline(&cmds_in_pipe);
    unsafe { LAST_EXIT }
}

fn do_test(argv: &[String]) -> i32 {
    let mut args: Vec<&str> = argv.iter().skip(1).map(|s| s.as_str()).collect();
    if argv[0] == "[" {
        if args.last().map(|s| *s == "]").unwrap_or(false) { args.pop(); }
        else { write(2, b"[: missing ]\n"); return 1; }
    }
    if args.is_empty() { return 1; }

    if args.len() == 3 {
        let (l, op, r) = (args[0], args[1], args[2]);
        return match op {
            "=" | "==" => { if l == r { 0 } else { 1 } }
            "!=" => { if l != r { 0 } else { 1 } }
            "-eq" => { let a: i32 = l.parse().unwrap_or(0); let b: i32 = r.parse().unwrap_or(0); if a == b { 0 } else { 1 } }
            "-ne" => { let a: i32 = l.parse().unwrap_or(0); let b: i32 = r.parse().unwrap_or(0); if a != b { 0 } else { 1 } }
            "-lt" => { let a: i32 = l.parse().unwrap_or(0); let b: i32 = r.parse().unwrap_or(0); if a < b { 0 } else { 1 } }
            "-le" => { let a: i32 = l.parse().unwrap_or(0); let b: i32 = r.parse().unwrap_or(0); if a <= b { 0 } else { 1 } }
            "-gt" => { let a: i32 = l.parse().unwrap_or(0); let b: i32 = r.parse().unwrap_or(0); if a > b { 0 } else { 1 } }
            "-ge" => { let a: i32 = l.parse().unwrap_or(0); let b: i32 = r.parse().unwrap_or(0); if a >= b { 0 } else { 1 } }
            _ => 1,
        };
    }
    if args.len() == 2 {
        let (flag, val) = (args[0], args[1]);
        return match flag {
            "-n" => { if !val.is_empty() { 0 } else { 1 } }
            "-z" => { if val.is_empty() { 0 } else { 1 } }
            "-f" => {
                let c = CString::new(val.as_bytes()).unwrap();
                let fd = open(c.as_ptr() as *const u8, 0);
                if fd < 0xFFFF_FFFF_FFFF_FF00 { close(fd); 0 } else { 1 }
            }
            "-d" => {
                let c = CString::new(val.as_bytes()).unwrap();
                let fd = open(c.as_ptr() as *const u8, 0);
                if fd < 0xFFFF_FFFF_FFFF_FF00 {
                    let mut st = [0u8; 144];
                    let ret = skyos_libc::syscall::fstat(fd, st.as_mut_ptr());
                    close(fd);
                    let st_mode = u32::from_ne_bytes(st[..4].try_into().unwrap_or([0; 4]));
                    if st_mode & 0x4000 != 0 { 0 } else { 1 }
                } else { 1 }
            }
            _ => 1,
        };
    }
    if args.len() == 1 {
        return if args[0].is_empty() { 1 } else { 0 };
    }
    1
}

fn exec_line(line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() { return; }

    for segment in trimmed.split(';') {
        let segment = segment.trim();
        if segment.is_empty() { continue; }

        if segment.starts_with("if ") {
            let rest = &segment[3..];
            let parts: Vec<&str> = rest.split("; then ").collect();
            if parts.len() == 2 {
                let cond = parts[0];
                let body = parts[1];
                let then_parts: Vec<&str> = body.splitn(2, "; else ").collect();
                let (true_body, false_body) = if then_parts.len() == 2 { (then_parts[0], Some(then_parts[1])) } else { (body, None) };
                let true_body = true_body.trim_end_matches("; fi").trim_end_matches(";fi");
                let false_body = false_body.map(|s| s.trim_end_matches("; fi").trim_end_matches(";fi"));

                let cond_exit = exec_segment(cond);
                if cond_exit == 0 {
                    for sub in true_body.split(';') { exec_segment(sub.trim()); }
                } else if let Some(fb) = false_body {
                    if fb != "fi" { for sub in fb.split(';') { exec_segment(sub.trim()); } }
                }
            }
            continue;
        }

        if segment.starts_with("for ") {
            let rest = &segment[4..];
            let parts: Vec<&str> = rest.splitn(2, " in ").collect();
            if parts.len() == 2 {
                let var = parts[0].trim();
                let body = parts[1];
                let body_parts: Vec<&str> = body.splitn(2, "; do ").collect();
                if body_parts.len() == 2 {
                    let list_str = body_parts[0].trim();
                    let loop_body = body_parts[1].trim_end_matches("; done").trim_end_matches(";done").trim();
                    let list: Vec<&str> = list_str.split(' ').filter(|s| !s.is_empty()).collect();
                    let orig_val = env_get(var);
                    for item in &list {
                        env_set(var, item);
                        for sub in loop_body.split(';') { exec_segment(sub.trim()); }
                    }
                    if let Some(ov) = orig_val { env_set(var, &ov); } else { env_set(var, ""); }
                }
            }
            continue;
        }

        if segment.starts_with("while ") || segment.starts_with("until ") {
            let is_until = segment.starts_with("until ");
            let rest = if is_until { &segment[6..] } else { &segment[6..] };
            let parts: Vec<&str> = rest.splitn(2, "; do ").collect();
            if parts.len() == 2 {
                let cond = parts[0].trim();
                let loop_body = parts[1].trim_end_matches("; done").trim_end_matches(";done").trim();
                loop {
                    let cond_exit = exec_segment(cond);
                    let should_stop = if is_until { cond_exit == 0 } else { cond_exit != 0 };
                    if should_stop { break; }
                    for sub in loop_body.split(';') { exec_segment(sub.trim()); }
                }
            }
            continue;
        }

        exec_segment(segment);
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    env_init();
    hist_load();
    loop {
        write(1, PROMPT);
        let mut buf = [0u8; MAX_LINE];
        let len = stdin_read(&mut buf);
        if len == usize::MAX {
            write(1, b"\n");
            continue;
        }
        if len == 0 { continue; }
        let line = core::str::from_utf8(&buf[..len]).unwrap_or("");
        exec_line(line);
    }
}
