use alloc::boxed::Box;

const BUF_SIZE: usize = 4096;

pub struct File {
    pub fd: u64,
    pub buf: [u8; BUF_SIZE],
    pub pos: usize,
    pub len: usize,
    pub eof: bool,
    pub error: bool,
    pub writeable: bool,
}

pub static mut STDIN: File = File {
    fd: 0,
    buf: [0; BUF_SIZE],
    pos: 0,
    len: 0,
    eof: false,
    error: false,
    writeable: false,
};

pub static mut STDOUT: File = File {
    fd: 1,
    buf: [0; BUF_SIZE],
    pos: 0,
    len: 0,
    eof: false,
    error: false,
    writeable: true,
};

pub static mut STDERR: File = File {
    fd: 2,
    buf: [0; BUF_SIZE],
    pos: 0,
    len: 0,
    eof: false,
    error: false,
    writeable: true,
};

fn flush_file(f: &mut File) {
    if !f.writeable || f.pos == 0 { return; }
    let buf = &f.buf[..f.pos];
    let ret = crate::syscall::write(f.fd, buf);
    if ret as usize != buf.len() {
        f.error = true;
    }
    f.pos = 0;
}

fn fill_file(f: &mut File) {
    if f.eof || f.error { return; }
    let buf = &mut f.buf;
    let ret = crate::syscall::read(f.fd, buf);
    if (ret as i64) <= 0 {
        f.eof = true;
        f.len = 0;
    } else {
        f.len = ret as usize;
    }
    f.pos = 0;
}

pub fn fopen(path: &str, mode: &str) -> Option<&'static mut File> {
    let cpath = alloc::ffi::CString::new(path).ok()?;
    let flags = if mode.contains('w') { 0x41 } else if mode.contains('a') { 0x200 } else { 0x0 };
    let fd = crate::syscall::open(cpath.as_ptr() as *const u8, flags);
    if (fd as i64) < 0 { return None; }

    let f = alloc::boxed::Box::new(File {
        fd,
        buf: [0; BUF_SIZE],
        pos: 0,
        len: 0,
        eof: false,
        error: false,
        writeable: mode.contains('w') || mode.contains('a') || mode.contains('+'),
    });
    Some(Box::leak(f))
}

pub fn fclose(file: &mut File) {
    if file.writeable {
        flush_file(file);
    }
    crate::syscall::close(file.fd);
}

pub fn fread(buf: &mut [u8], file: &mut File) -> usize {
    let mut total = 0;
    while total < buf.len() {
        if file.pos >= file.len {
            if file.eof { break; }
            fill_file(file);
            if file.len == 0 { break; }
        }
        let avail = core::cmp::min(file.len - file.pos, buf.len() - total);
        buf[total..total + avail].copy_from_slice(&file.buf[file.pos..file.pos + avail]);
        total += avail;
        file.pos += avail;
    }
    total
}

pub fn fwrite(buf: &[u8], file: &mut File) -> usize {
    if !file.writeable { return 0; }
    let mut written = 0;
    while written < buf.len() {
        let space = BUF_SIZE - file.pos;
        if space == 0 {
            flush_file(file);
            continue;
        }
        let avail = core::cmp::min(space, buf.len() - written);
        file.buf[file.pos..file.pos + avail].copy_from_slice(&buf[written..written + avail]);
        file.pos += avail;
        written += avail;
    }
    written
}

pub fn fprintf(file: &mut File, format: &str, args: &[u64]) -> usize {
    let mut written = 0;
    let mut arg_idx = 0;
    let bytes = format.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b's' => {
                    if arg_idx < args.len() {
                        let s = unsafe { core::ffi::CStr::from_ptr(args[arg_idx] as *const i8) };
                        let s = s.to_bytes();
                        written += fwrite(s, file);
                        arg_idx += 1;
                    }
                }
                b'd' | b'i' => {
                    if arg_idx < args.len() {
                        let val = args[arg_idx] as i64;
                        let mut s = alloc::vec::Vec::new();
                        if val == 0 {
                            s.push(b'0');
                        } else {
                            let mut n = if val < 0 { -val } else { val };
                            while n > 0 {
                                s.push(b'0' + (n % 10) as u8);
                                n /= 10;
                            }
                            if val < 0 { s.push(b'-'); }
                            s.reverse();
                        }
                        written += fwrite(&s, file);
                        arg_idx += 1;
                    }
                }
                b'u' => {
                    if arg_idx < args.len() {
                        let val = args[arg_idx];
                        let mut s = alloc::vec::Vec::new();
                        if val == 0 {
                            s.push(b'0');
                        } else {
                            let mut n = val;
                            while n > 0 {
                                s.push(b'0' + (n % 10) as u8);
                                n /= 10;
                            }
                            s.reverse();
                        }
                        written += fwrite(&s, file);
                        arg_idx += 1;
                    }
                }
                b'x' | b'X' => {
                    if arg_idx < args.len() {
                        let val = args[arg_idx];
                        let hex = b"0123456789abcdef";
                        let mut s = alloc::vec::Vec::new();
                        s.push(b'0');
                        s.push(b'x');
                        let mut found = false;
                        for shift in (0..64).rev().step_by(4) {
                            let nibble = (val >> shift) & 0xF;
                            if nibble != 0 || found {
                                found = true;
                                s.push(hex[nibble as usize]);
                            }
                        }
                        if !found { s.push(b'0'); }
                        written += fwrite(&s, file);
                        arg_idx += 1;
                    }
                }
                b'%' => {
                    written += fwrite(b"%", file);
                }
                _ => {}
            }
            i += 1;
        } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'n' => { written += fwrite(b"\n", file); }
                b't' => { written += fwrite(b"\t", file); }
                b'r' => { written += fwrite(b"\r", file); }
                _ => { written += fwrite(&[bytes[i]], file); }
            }
            i += 1;
        } else {
            written += fwrite(&[bytes[i]], file);
            i += 1;
        }
    }
    flush_file(file);
    written
}

pub fn printf(format: &str, args: &[u64]) -> usize {
    unsafe { fprintf(&mut *(&raw mut STDOUT as *mut File), format, args) }
}

pub fn eprintf(format: &str, args: &[u64]) -> usize {
    unsafe { fprintf(&mut *(&raw mut STDERR as *mut File), format, args) }
}

pub fn flush_stdout() {
    unsafe { flush_file(&mut *(&raw mut STDOUT as *mut File)); }
}

pub fn flush_stderr() {
    unsafe { flush_file(&mut *(&raw mut STDERR as *mut File)); }
}
