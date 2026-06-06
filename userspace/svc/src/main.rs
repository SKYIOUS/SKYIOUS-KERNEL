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
use skyos_libc::syscall::{write, exit, fork, execve, open, close, read, wait4, kill, getdents64};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
}

const PROC_DIR: &[u8] = b"/proc/\0";

fn eprint(s: &str) {
    let _ = write(2, s.as_bytes());
}

fn format_pid_list() -> Vec<(i64, String)> {
    let dir_fd = open(PROC_DIR.as_ptr(), 0);
    if dir_fd >= 0xFFFF_FFFF_FFFF_FF00 {
        return vec![];
    }
    let mut buf = [0u8; 4096];
    let n = getdents64(dir_fd, buf.as_mut_ptr(), buf.len());
    close(dir_fd);
    if n >= 0xFFFF_FFFF_FFFF_FF00 {
        return vec![];
    }
    let mut result = vec![];
    let mut off = 0;
    let n = n as usize;
    while off + 18 < n {
        let _d_ino = u64::from_ne_bytes(buf[off..off+8].try_into().unwrap());
        let _d_off = u64::from_ne_bytes(buf[off+8..off+16].try_into().unwrap());
        let d_reclen = u16::from_ne_bytes(buf[off+16..off+18].try_into().unwrap()) as usize;
        if d_reclen < 19 || off + d_reclen > n {
            break;
        }
        let name_end = off + 18 + buf[off+18..off+d_reclen].iter().position(|&b| b == 0).unwrap_or(d_reclen - 19);
        let name = core::str::from_utf8(&buf[off+18..name_end]).unwrap_or("");
        let _d_type = buf[off + d_reclen - 1];
        if let Ok(pid) = name.parse::<i64>() {
            let mut cmdline = [0u8; 256];
            let cmd_path_str = alloc::format!("/proc/{}/cmdline\0", pid);
            let cmd_bytes = cmd_path_str.as_bytes();
            let fd = open(cmd_bytes.as_ptr(), 0);
            if fd < 0xFFFF_FFFF_FFFF_FF00 {
                let n2 = read(fd, &mut cmdline);
                close(fd);
                if n2 > 0 {
                    let cmd = core::str::from_utf8(&cmdline[..n2 as usize]).unwrap_or("").trim_end_matches('\0');
                    result.push((pid, cmd.to_string()));
                }
            }
        }
        off += d_reclen;
        if _d_off == 0 {
            break;
        }
    }
    result
}

fn cmd_status() {
    let procs = format_pid_list();
    eprint("PID   COMMAND\n");
    for (pid, cmd) in &procs {
        let line = alloc::format!("{:<5} {}\n", pid, cmd);
        eprint(&line);
    }
}

fn cmd_start(path: &str) {
    let pid = fork();
    if pid == 0 {
        let c = CString::new(path).unwrap();
        let argv: [u64; 2] = [c.as_ptr() as u64, 0];
        let envp: [u64; 1] = [0];
        let _ = execve(c.as_ptr() as *const u8, argv.as_ptr() as *const *const u8, envp.as_ptr() as *const *const u8);
        exit(1);
    } else if pid > 0 && pid < 0xFFFF_FFFF_FFFF_FF00 {
        let mut status: i32 = 0;
        wait4(pid as i64, &mut status, 0, core::ptr::null_mut());
        eprint("[svc] started\n");
    }
}

fn cmd_stop(path: &str) {
    let procs = format_pid_list();
    for (pid, cmd) in &procs {
        if cmd == path {
            kill(*pid, 15);
            let mut status: i32 = 0;
            wait4(*pid, &mut status, 0, core::ptr::null_mut());
            eprint("[svc] stopped\n");
            return;
        }
    }
    eprint("[svc] not found\n");
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let argv = unsafe {
        if _argv.is_null() {
            return 1;
        }
        let mut i = 0;
        let mut args = vec![];
        while !(*_argv.add(i)).is_null() {
            let cstr = core::ffi::CStr::from_ptr(*_argv.add(i) as *const i8);
            args.push(cstr.to_str().unwrap_or(""));
            i += 1;
        }
        args
    };

    if argv.len() < 2 {
        eprint("Usage: svc <status|start|stop|restart> [path]\n");
        return 1;
    }

    match argv[1] {
        "status" | "list" | "ls" => cmd_status(),
        "start" => {
            if argv.len() < 3 { eprint("missing path\n"); return 1; }
            cmd_start(argv[2]);
        }
        "stop" => {
            if argv.len() < 3 { eprint("missing path\n"); return 1; }
            cmd_stop(argv[2]);
        }
        "restart" => {
            if argv.len() < 3 { eprint("missing path\n"); return 1; }
            cmd_stop(argv[2]);
            cmd_start(argv[2]);
        }
        _ => {
            eprint("unknown command\n");
            return 1;
        }
    }
    0
}
