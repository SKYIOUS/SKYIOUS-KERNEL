use crate::{print, println};
use crate::vga_buffer::{self, Color};
use alloc::format;

pub fn ls(path: &str) {
    let p = if path.is_empty() { "." } else { path };
    if let Some(node) = crate::vfs::VFS.lock().resolve_path(p) {
        if node.is_dir() {
            if let Ok(children) = node.children() {
                for child in children {
                    if child.is_dir() {
                        vga_buffer::set_color(Color::LightBlue, Color::Black);
                        print!("{}/", child.name());
                        vga_buffer::set_color(Color::White, Color::Black);
                        print!("  ");
                    } else {
                        print!("{}  ", child.name());
                    }
                }
                println!();
            }
        } else {
            println!("{}", node.name());
        }
    } else {
        println!("ls: {}: No such file or directory", p);
    }
}

pub fn cd(path: &str) {
    if path.is_empty() { return; }
    let p = format!("{}\0", path);
    if crate::syscalls::syscall_handler(80, p.as_ptr() as u64, 0, 0, 0, 0, core::ptr::null_mut()) != 0 {
        println!("cd failed: No such directory");
    }
}

pub fn pwd() {
    let mut buf = [0u8; 256];
    let res = crate::syscalls::syscall_handler(79, buf.as_mut_ptr() as u64, 256, 0, 0, 0, core::ptr::null_mut());
    if res != 0 && res < 0x8000_0000_0000_0000 {
        if let Ok(path) = core::str::from_utf8(&buf) {
            println!("{}", path.trim_matches(char::from(0)));
        }
    }
}

pub fn mkdir(path: &str) {
    if path.is_empty() { return; }
    let p = format!("{}\0", path);
    if crate::syscalls::syscall_handler(83, p.as_ptr() as u64, 0o755, 0, 0, 0, core::ptr::null_mut()) != 0 {
        println!("mkdir failed");
    }
}

pub fn rm(path: &str) {
    if path.is_empty() { return; }
    let p = format!("{}\0", path);
    if crate::syscalls::syscall_handler(87, p.as_ptr() as u64, 0, 0, 0, 0, core::ptr::null_mut()) != 0 {
        println!("rm failed");
    }
}

pub fn touch(path: &str) {
    if path.is_empty() { return; }
    let p = format!("{}\0", path);
    let fd = crate::syscalls::syscall_handler(2, p.as_ptr() as u64, 0x40, 0, 0, 0, core::ptr::null_mut());
    if fd < 1000 {
        crate::syscalls::syscall_handler(3, fd, 0, 0, 0, 0, core::ptr::null_mut()); // Close
    } else {
        println!("touch failed");
    }
}

pub fn cat(filename: &str) {
    if filename.is_empty() { return; }
    match crate::syscalls::sys_open_path(filename) {
        Ok(fd) => {
            let mut buf = [0u8; 4096];
            loop {
                let read_len = crate::syscalls::syscall_handler(0, fd, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, core::ptr::null_mut()); // SYS_READ=0
                if read_len == 0 || read_len >= 0xFFFF_FFFF_FFFF_FF00 {
                    break;
                }
                if let Ok(content) = core::str::from_utf8(&buf[..read_len as usize]) {
                    print!("{}", content);
                }
            }
            crate::syscalls::syscall_handler(3, fd, 0, 0, 0, 0, core::ptr::null_mut()); // SYS_CLOSE=3
            println!();
        }
        Err(e) => {
             println!("cat: failed to open {}: {:?}", filename, e);
        }
    }
}

pub fn stat(filename: &str) {
    if filename.is_empty() { return; }
    let path = if filename.starts_with('/') {
        format!("{}\0", filename)
    } else {
        format!("/{}\0", filename)
    };
    
    let mut stat_buf = crate::vfs::Stat {
        st_dev: 0, st_ino: 0, st_mode: 0, st_nlink: 0, st_uid: 0, 
        st_gid: 0, st_rdev: 0, st_size: 0, st_atime: 0, st_mtime: 0, st_ctime: 0
    };
    
    let res = crate::syscalls::syscall_handler(4, path.as_ptr() as u64, &mut stat_buf as *mut _ as u64, 0, 0, 0, core::ptr::null_mut()); // SYS_STAT
    if res == 0 {
        println!("File: {}", filename);
        println!("Size: {} bytes", stat_buf.st_size);
        println!("Mode: {:o}", stat_buf.st_mode);
    } else {
        println!("stat failed: File not found");
    }
}

pub fn cp(src: &str, dst: &str) {
    if src.is_empty() || dst.is_empty() {
        println!("Usage: cp <source> <dest>");
        return;
    }
    let src_c = format!("{}\0", src);
    let dst_c = format!("{}\0", dst);
    
    let fd_src = crate::syscalls::syscall_handler(2, src_c.as_ptr() as u64, 0, 0, 0, 0, core::ptr::null_mut());
    if fd_src >= 0xFFFF_FFFF_FFFF_FF00 {
        println!("cp: failed to open source {}", src);
    } else {
        let fd_dst = crate::syscalls::syscall_handler(2, dst_c.as_ptr() as u64, 0x41, 0, 0, 0, core::ptr::null_mut()); // O_CREAT|O_WRONLY
        if fd_dst >= 0xFFFF_FFFF_FFFF_FF00 {
            println!("cp: failed to create destination {}", dst);
            crate::syscalls::syscall_handler(3, fd_src, 0, 0, 0, 0, core::ptr::null_mut());
        } else {
            let mut buf = [0u8; 4096];
            loop {
                let n = crate::syscalls::syscall_handler(0, fd_src, buf.as_mut_ptr() as u64, 4096, 0, 0, core::ptr::null_mut());
                if n == 0 || n >= 0xFFFF_FFFF_FFFF_FF00 { break; }
                crate::syscalls::syscall_handler(1, fd_dst, buf.as_ptr() as u64, n, 0, 0, core::ptr::null_mut());
            }
            crate::syscalls::syscall_handler(3, fd_src, 0, 0, 0, 0, core::ptr::null_mut());
            crate::syscalls::syscall_handler(3, fd_dst, 0, 0, 0, 0, core::ptr::null_mut());
            println!("cp: copied {} to {}", src, dst);
        }
    }
}

pub fn mount() {
    println!("Current Mount Points:");
    println!("  /      (ramfs)");
}
