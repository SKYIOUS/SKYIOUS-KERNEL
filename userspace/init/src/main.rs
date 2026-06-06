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
use skyos_libc::syscall::{write, exit, fork, execve, wait4, open, close, read, mount};

const MAX_SERVICES: usize = 32;
const MAX_RESTARTS: usize = 5;
const INIT_CFG: &[u8] = b"/etc/init.cfg\0";

#[derive(Clone, Copy, PartialEq)]
enum ServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}

struct Service {
    path: String,
    pid: i64,
    state: ServiceState,
    restarts: usize,
    max_restarts: usize,
}

static mut SERVICES: Vec<Service> = vec![];
static mut SHELL_PID: i64 = 0;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    exit(1);
}

fn print(s: &str) {
    let _ = write(1, s.as_bytes());
}

fn eprint(s: &str) {
    let _ = write(2, s.as_bytes());
}

fn read_file(path: *const u8) -> Option<Vec<u8>> {
    let fd = open(path, 0);
    if fd >= 0xFFFF_FFFF_FFFF_FF00 {
        return None;
    }
    let mut buf = vec![0u8; 4096];
    let n = read(fd, &mut buf);
    close(fd);
    if n >= 0xFFFF_FFFF_FFFF_FF00 {
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

fn mount_fstab() {
    let fstab_path = b"/etc/fstab\0";
    if let Some(data) = read_file(fstab_path.as_ptr()) {
        let s = core::str::from_utf8(&data).unwrap_or("");
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }
            let source = parts[0];
            let target = parts[1];
            let fstype = parts[2];
            let source_c = CString::new(source).ok();
            let target_c = CString::new(target).ok();
            let fstype_c = CString::new(fstype).ok();
            if let (Some(src), Some(tgt), Some(fs)) = (source_c, target_c, fstype_c) {
                let ret = mount(src.as_ptr() as *const u8, tgt.as_ptr() as *const u8, fs.as_ptr() as *const u8, 0);
                if ret >= 0xFFFF_FFFF_FFFF_FF00 {
                    eprint("[init] fstab mount failed: ");
                    eprint(source);
                    eprint(" -> ");
                    eprint(target);
                    eprint("\n");
                } else {
                    print("[init] fstab mounted ");
                    print(source);
                    print(" -> ");
                    print(target);
                    print("\n");
                }
            }
        }
    }
}

fn parse_init_cfg(data: &[u8]) {
    let s = core::str::from_utf8(data).unwrap_or("");
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "fstab" => {
                mount_fstab();
            }
            "mount" if parts.len() >= 4 => {
                let source = parts[1];
                let target = parts[2];
                let fstype = parts[3];
                let source_c = CString::new(source).ok();
                let target_c = CString::new(target).ok();
                let fstype_c = CString::new(fstype).ok();
                if let (Some(src), Some(tgt), Some(fs)) = (source_c, target_c, fstype_c) {
                    let ret = mount(src.as_ptr() as *const u8, tgt.as_ptr() as *const u8, fs.as_ptr() as *const u8, 0);
                    if ret >= 0xFFFF_FFFF_FFFF_FF00 {
                        eprint("[init] mount failed: ");
                        eprint(source);
                        eprint(" -> ");
                        eprint(target);
                        eprint("\n");
                    } else {
                        print("[init] mounted ");
                        print(source);
                        print(" on ");
                        print(target);
                        print("\n");
                    }
                }
            }
            "service" if parts.len() >= 2 => {
                let path = parts[1].to_string();
                let max_r = if parts.len() >= 3 {
                    parts[2].parse::<usize>().unwrap_or(MAX_RESTARTS)
                } else {
                    MAX_RESTARTS
                };
                unsafe {
                    if SERVICES.len() < MAX_SERVICES {
                        SERVICES.push(Service {
                            path,
                            pid: 0,
                            state: ServiceState::Stopped,
                            restarts: 0,
                            max_restarts: max_r,
                        });
                    }
                }
            }
            "login" if parts.len() >= 3 => {
                let _tty = parts[1];
                let shell = parts[2];
                let pid = fork();
                if pid == 0 {
                    let shell_c = CString::new(shell).ok();
                    if let Some(s) = shell_c {
                        let argv: [u64; 2] = [s.as_ptr() as u64, 0];
                        let envp: [u64; 1] = [0];
                        let _ = execve(s.as_ptr() as *const u8, argv.as_ptr() as *const *const u8, envp.as_ptr() as *const *const u8);
                    }
                    exit(1);
                } else if pid > 0 && pid < 0xFFFF_FFFF_FFFF_FF00 {
                    print("[init] login on ");
                    print(_tty);
                    print("\n");
                }
            }
            _ => {
                eprint("[init] unknown directive: ");
                eprint(parts[0]);
                eprint("\n");
            }
        }
    }
}

fn start_service(idx: usize) {
    unsafe {
        let svc = &mut SERVICES[idx];
        let pid = fork();
        if pid == 0 {
            let path_c = CString::new(svc.path.as_str()).unwrap();
            let argv: [u64; 2] = [path_c.as_ptr() as u64, 0];
            let envp: [u64; 1] = [0];
            let _ = execve(path_c.as_ptr() as *const u8, argv.as_ptr() as *const *const u8, envp.as_ptr() as *const *const u8);
            exit(1);
        } else if pid > 0 && pid < 0xFFFF_FFFF_FFFF_FF00 {
            svc.pid = pid as i64;
            svc.state = ServiceState::Running;
            print("[init] started ");
            print(&svc.path);
            print("\n");
        }
    }
}

fn start_all_services() {
    unsafe {
        for i in 0..SERVICES.len() {
            start_service(i);
        }
    }
}

fn start_login_shell() {
    let pid = fork();
    if pid == 0 {
        let shell_c = CString::new("/bin/login").unwrap();
        let argv: [u64; 2] = [shell_c.as_ptr() as u64, 0];
        let envp: [u64; 1] = [0];
        let _ = execve(shell_c.as_ptr() as *const u8, argv.as_ptr() as *const *const u8, envp.as_ptr() as *const *const u8);
        exit(1);
    } else if pid > 0 && pid < 0xFFFF_FFFF_FFFF_FF00 {
        unsafe {
            SHELL_PID = pid as i64;
        }
        print("[init] login shell started\n");
    }
}

fn reaper_loop() -> ! {
    loop {
        let mut status: i32 = 0;
        let pid = wait4(-1, &mut status, 0, core::ptr::null_mut());
        if pid >= 0xFFFF_FFFF_FFFF_FF00 {
            continue;
        }
        let pid = pid as i64;
        let exited = (status & 0x7f) == 0;
        let code = (status >> 8) & 0xff;
        let _signaled = (status & 0x7f) != 0;
        let sig = status & 0x7f;

        unsafe {
            if SHELL_PID == pid {
                print("[init] shell exited, respawning\n");
                SHELL_PID = 0;
                start_login_shell();
                continue;
            }
            for i in 0..SERVICES.len() {
                let svc = &mut SERVICES[i];
                if svc.pid == pid {
                    svc.pid = 0;
                    if exited && code == 0 {
                        svc.state = ServiceState::Stopped;
                        svc.restarts = 0;
                    } else {
                        svc.state = ServiceState::Crashed;
                        svc.restarts += 1;
                        if _signaled {
                            eprint(&alloc::format!("[init] {} crashed (signal {}), restart {}/{}\n",
                                svc.path, sig, svc.restarts, svc.max_restarts));
                        } else {
                            eprint(&alloc::format!("[init] {} crashed (exit {}), restart {}/{}\n",
                                svc.path, code, svc.restarts, svc.max_restarts));
                        }
                        if svc.restarts < svc.max_restarts {
                            start_service(i);
                        } else {
                            eprint(&alloc::format!("[init] {}: max restarts reached\n", svc.path));
                        }
                    }
                    break;
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    print("[init] SkyOS Phase 2 init\n");

    if let Some(data) = read_file(INIT_CFG.as_ptr()) {
        parse_init_cfg(&data);
    } else {
        print("[init] no /etc/init.cfg, using defaults\n");
    }

    start_all_services();
    start_login_shell();
    reaper_loop();
}
