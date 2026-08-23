#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
//! Process lifecycle syscalls: fork, clone, execve, exit, wait, sched, time.
//! Extracted from process.rs to keep each module under 1k lines.

use super::errno;
use super::numbers;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::vec;
use x86_64::VirtAddr;
use x86_64::structures::paging::{Page, Size4KiB, Mapper, FrameAllocator, PageTableFlags};
use crate::gdt;
use crate::interrupts::IrqFmtBuf;

pub fn sys_getppid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        p.parent_id.unwrap_or(0)
    } else {
        0
    }
}

pub fn sys_uname(buf: *mut UtsName) -> u64 {
    let mut uts = UtsName {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };

    let fill = |dest: &mut [u8; 65], src: &str| {
        let bytes = src.as_bytes();
        let len = core::cmp::min(bytes.len(), 64);
        dest[..len].copy_from_slice(&bytes[..len]);
    };

    fill(&mut uts.sysname, "Vahi");
    fill(&mut uts.nodename, "sarga-os");
    fill(&mut uts.release, "0.3.0");
    fill(&mut uts.version, "SARGA OS Ã¢â‚¬â€ Vahi V5.0 Roadmap Implementation");
    #[cfg(not(target_arch = "aarch64"))]
    fill(&mut uts.machine, "x86_64");
    #[cfg(target_arch = "aarch64")]
    fill(&mut uts.machine, "aarch64");

    if unsafe { user_access::copy_to_user(buf as *mut u8, core::slice::from_raw_parts(&uts as *const _ as *const u8, core::mem::size_of::<UtsName>())) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

pub fn sys_exit(status: u64) -> u64 {
    let (parent_pid, clear_tid) = {
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            *process.exit_code.lock() = Some(status as i32);
            if status != 42 {
                crate::println!("[PROCESS] Pid {} exited with status {}", process.id, status);
            }
            (process.parent_id, *process.clear_child_tid.lock())
        } else {
            (None, 0)
        }
    };
    
    // Clear child tid and wake futex (for pthread_join)
    if clear_tid != 0 {
        let zero = 0u32;
        let _ = unsafe { user_access::copy_to_user(clear_tid as *mut u8, core::slice::from_raw_parts(&zero as *const _ as *const u8, 4)) };
        let _ = crate::syscalls::futex::sys_futex(clear_tid as *mut u32, 1, 1, 0, core::ptr::null_mut(), 0);
    }
    
    // Send SIGCHLD to parent process
    if let Some(ppid) = parent_pid {
        let table = crate::task::process::PROCESS_TABLE.lock();
        if let Some(parent) = table.get(&ppid) {
            parent.signals.lock().raise(crate::syscalls::signal::Signal::SIGCHLD);
        }
        drop(table);
    }
    
    // Mark current thread as exited
    crate::task::scheduler::with_current_thread(|thread| {
        thread.status = crate::task::thread::ThreadStatus::Exited;
    });
    crate::task::scheduler::schedule();
    // The thread is Exited; schedule() switches away and frees the stack.
    // If it ever returned, idle-wait rather than returning from sys_exit.
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

pub fn sys_set_tid_address(tidptr: *const u32) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *lock {
        *proc.clear_child_tid.lock() = tidptr as u64;
        proc.id
    } else {
        0
    }
}

pub fn sys_exit_group(status: u64) -> u64 {
    crate::println!("[PROCESS] Thread group exited with {}", status);
    sys_exit(status)
}

pub fn sys_nanosleep(seconds: u64, nanoseconds: u64) -> u64 {
    // 1 tick = 1 timer interrupt. Assuming 100Hz = 10ms per tick.
    let ms = (seconds * 1000) + (nanoseconds / 1_000_000);
    let sleep_ticks = core::cmp::max(1, ms / 10);

    if check_signal_interrupt() { return errno::Errno::EINTR as u64; }

    let target_tick = crate::interrupts::get_ticks() + sleep_ticks;

    // Mark the current thread Blocked in place Ã¢â‚¬â€ do NOT take it out of
    // `current_thread`. `prepare_switch` saves the block-point context into
    // the thread's own `stack_ptr`, and when woken the thread resumes inside
    // `schedule()` and returns here Ã¢â€ â€™ syscall postamble Ã¢â€ â€™ sysretq.
    {
        let mut sched = crate::task::scheduler::this_cpu_sched().lock();
        if let Some(current) = sched.current_thread.as_mut() {
            current.status = crate::task::thread::ThreadStatus::Blocked;
            current.sleep_until = Some(target_tick);
        }
    }

    crate::task::scheduler::schedule();
    0
}

pub fn sys_sysinfo(buf: *mut u64) -> u64 {
    let uptime_ticks = crate::interrupts::get_ticks();
    let uptime_secs = uptime_ticks / 100;
    let info = [
        0u64,                            // total_ram (pages)
        0u64,                            // free_ram (pages)
        uptime_secs,                     // uptime_seconds
        0u64,                            // processes
        1u64,                            // load_avg_1m (1<<16 fixed point)
    ];
    if unsafe { crate::syscalls::user_access::copy_to_user(
        buf as *mut u8,
        core::slice::from_raw_parts(
            info.as_ptr() as *const u8,
            info.len() * 8,
        ),
    ) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

pub fn sys_arch_prctl(code: u64, addr: u64) -> u64 {
    use x86_64::instructions::segmentation::Segment64;
    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_GET_FS: u64 = 0x1003;

    match code {
        ARCH_SET_FS => {
            unsafe {
                x86_64::registers::segmentation::FS::write_base(x86_64::VirtAddr::new(addr));
            }
            0
        }
        ARCH_GET_FS => {
            let base = x86_64::registers::segmentation::FS::read_base();
            if addr != 0 {
                let val = base.as_u64();
                if unsafe { user_access::copy_to_user(addr as *mut u8, core::slice::from_raw_parts(&val as *const _ as *const u8, 8)) }.is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
            0
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

pub fn sys_sched_yield() -> u64 {
    use crate::task::scheduler;
    let switch = {
        let mut sched = scheduler::this_cpu_sched().lock();
        sched.prepare_switch_tls()
    };
    if let Some((old_ptr, new_sp, new_fs)) = switch {
        crate::task::thread::switch_thread(old_ptr, new_sp, new_fs);
        // The yielded thread was parked in `switching_old` by
        // prepare_switch; the post-switch drain doesn't run inside a
        // syscall (we return to userland directly), so reclaim it here.
        let mut sched = scheduler::this_cpu_sched().lock();
        scheduler::route_switching_old(&mut sched);
    }
    0
}

pub fn sys_sched_setattr(pid: i64, attr_ptr: *const u8, _flags: u64) -> u64 {
    let proc = if pid == 0 {
        let lock = crate::task::process::CURRENT_PROCESS.lock();
        match *lock {
            Some(ref p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    } else {
        // ponytail: table lock dropped after clone; Arc holds refcount so this is safe
        let table = crate::task::process::PROCESS_TABLE.lock();
        match table.get(&(pid as u64)) {
            Some(p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };

    if attr_ptr.is_null() { return errno::Errno::EFAULT as u64; }

    let size = unsafe { *(attr_ptr as *const u32) };
    if size < 8 { return errno::Errno::EINVAL as u64; }

    let policy = unsafe { *(attr_ptr.add(4) as *const u32) };
    if policy != 0 { return errno::Errno::EINVAL as u64; } // Only SCHED_OTHER

    let nice = if size >= 12 {
        unsafe { *(attr_ptr.add(8) as *const i32) }
    } else {
        0
    };

    // Map nice [-20..19] to priority [0..7]
    let priority = if nice <= -15 { 7u8 }
        else if nice <= -10 { 6u8 }
        else if nice <= -5  { 5u8 }
        else if nice <= 0   { 4u8 }
        else if nice <= 5   { 3u8 }
        else if nice <= 10  { 2u8 }
        else if nice <= 15  { 1u8 }
        else { 0u8 };

    // Update current thread priority if it belongs to the target process
    let mut sched = crate::task::scheduler::this_cpu_sched().lock();
    if let Some(ref mut cur) = sched.current_thread {
        if let Some(ref p) = cur.process {
            if p.id == proc.id {
                cur.priority = priority;
            }
        }
    }
    drop(sched);

    // Update global pending queue threads
    let mut pend = crate::task::scheduler::GLOBAL.pending_queue.lock();
    for t in pend.iter_mut() {
        if let Some(ref p) = t.process {
            if p.id == proc.id {
                t.priority = priority;
            }
        }
    }
    drop(pend);
    0
}

pub fn sys_sched_getattr(pid: i64, attr_ptr: *mut u8, size: u64, _flags: u64) -> u64 {
    let target = if pid == 0 {
        let lock = crate::task::process::CURRENT_PROCESS.lock();
        match *lock {
            Some(ref p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    } else {
        let table = crate::task::process::PROCESS_TABLE.lock();
        match table.get(&(pid as u64)) {
            Some(p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };

    if attr_ptr.is_null() { return errno::Errno::EFAULT as u64; }
    let out_size = if size == 0 { 12u32 } else { size as u32 };

    // Get current thread priority if it belongs to target process
    let priority = {
        let sched = crate::task::scheduler::this_cpu_sched().lock();
        if let Some(ref cur) = sched.current_thread {
            if let Some(ref p) = cur.process {
                if p.id == target.id { cur.priority } else { 3u8 }
            } else { 3u8 }
        } else { 3u8 }
    };

    let nice = match priority {
        7 => -20, 6 => -10, 5 => -5, 4 => 0,
        3 => 5, 2 => 10, 1 => 15, _ => 19,
    };

        if unsafe { user_access::copy_to_user(attr_ptr, core::slice::from_raw_parts(&out_size as *const _ as *const u8, 4)) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    if out_size >= 8 {
        let zero = 0u32;
        if unsafe { user_access::copy_to_user(attr_ptr.add(4), core::slice::from_raw_parts(&zero as *const _ as *const u8, 4)) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    if out_size >= 12 {
        let nice_le = nice as u32;
        if unsafe { user_access::copy_to_user(attr_ptr.add(8), core::slice::from_raw_parts(&nice_le as *const _ as *const u8, 4)) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    0
}

pub fn sys_fork(regs_ptr: *mut u64) -> u64 {
    use crate::task::process::{Process, CURRENT_PROCESS};
    use crate::memory::buddy::BuddyFrameAllocator;

    crate::serial_write("[FORK] enter\n");
    let parent_lock = CURRENT_PROCESS.lock();
    if let Some(ref parent) = *parent_lock {
        // Cgroup pids.max check
        {
            let cg_path = parent.cgroup_path.lock();
            let hierarchy = crate::syscalls::cgroup::cgroup_ensure();
            if let Some(cg) = hierarchy.find_cgroup(&cg_path) {
                if !cg.can_fork() {
                    crate::serial_write("[FORK] denied: cgroup pids.max reached\n");
                    return errno::Errno::EAGAIN as u64;
                }
            }
        }
        let parent_id = parent.id;
        // 1. Clone Address Space with CoW
        let mut frame_allocator = BuddyFrameAllocator;
        let child_as = match parent.address_space.clone_cow(&mut frame_allocator) {
            Some(as_space) => as_space,
            None => return errno::Errno::ENOMEM as u64,
        };
        crate::serial_write("[FORK] cow done\n");
        // FORKDIAG: dump the parent's slab free-list heads (ALLOCATOR .bss at
        // the init binary's 0x4063d8) with VMA residency, to see whether a
        // free-list entry already points at an unmapped page pre-fork.
        {
            let mut scratch = [0u8; 1024];
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let _ = core::fmt::write(&mut w, format_args!("[FORKDIAG] pid={} heads:", parent_id));
            for cls in 0..9usize {
                let addr = 0x4063d8u64 + (cls as u64) * 8;
                let pv = crate::memory::virt_to_phys(x86_64::VirtAddr::new(addr));
                let head = match pv {
                    Some(phys) => {
                        let k = crate::memory::physical_memory_offset() + phys.as_u64();
                        unsafe { *(k as *const u64) }
                    }
                    None => u64::MAX,
                };
                let in_vma = head != 0 && head != u64::MAX
                    && parent.find_vma(head).is_some();
                let _ = core::fmt::write(&mut w, format_args!(" c{}={:#x}{}", cls, head, if head != 0 && head != u64::MAX { if in_vma { "v" } else { "X" } } else { "" }));
            }
            let _ = core::fmt::write(&mut w, format_args!("
"));
            let diag_len = w.len;
            drop(w);
            crate::serial_write(core::str::from_utf8(&scratch[..diag_len]).unwrap_or(""));
        }

        // 2. Create new Process
        let child_pid = Process::next_id();
        let mut child_process = Process::new(child_pid, Some(parent_id), child_as);
        {
            let parent_vmas = parent.memory.lock().vmas.clone();
            child_process.memory.lock().vmas = parent_vmas;
        }
        child_process.entry_point = parent.entry_point;
        child_process.files.lock().fd_table = parent.files.lock().fd_table.clone();
        child_process.files.lock().fd_flags = parent.files.lock().fd_flags.clone();
        child_process.files.lock().dir_fds = parent.files.lock().dir_fds.clone();
        child_process.clone_credentials_from(parent);
        {
            let p_id = parent.identity.lock();
            let mut c_id = child_process.identity.lock();
            c_id.pgid = p_id.pgid;
            c_id.session = p_id.session;
            c_id.is_group_leader = false;
        }
        // Copy the brk pointer: the child heap region must mirror the parent
        // or demand-paging of inherited brk pages SIGSEGVs.
        child_process.memory.lock().brk = parent.memory.lock().brk;
        let child_arc = Arc::new(child_process);
        crate::serial_write("[FORK] process cloned\n");

        // 3. Clone current thread (deep copy stack) BEFORE registering the
        // child, so a stack-alloc failure leaves no orphan in children/table.
        let child_thread = {
            let sched = crate::task::scheduler::this_cpu_sched().lock();
            match sched.current_thread.as_ref() {
                Some(t) => match t.clone_fork(child_arc.clone(), regs_ptr) {
                    Some(t) => t,
                    None => return errno::Errno::ENOMEM as u64,
                },
                None => {
                    crate::serial_write("[FORK] no current thread!\n");
                    return errno::Errno::EPERM as u64;
                }
            }
        };
        crate::serial_write("[FORK] thread cloned\n");

        // Track child in parent and global table
        parent.children.lock().push(child_pid);
        crate::task::process::Process::register(child_arc.clone());
        crate::serial_write("[FORK] registered\n");

        // 4. Add to scheduler
        crate::task::scheduler::spawn_thread(child_thread);
        crate::serial_write("[FORK] spawned\n");

        return child_pid;
    }
    crate::serial_write("[FORK] no current process!\n");
    
    errno::Errno::EPERM as u64 
}

pub fn sys_clone(flags: u64, child_stack: u64, parent_tid: *mut u32, child_tls: u64, child_tidptr: *mut u32, regs_ptr: *mut u64) -> u64 {
    use crate::task::process::{Process, CURRENT_PROCESS};
    use crate::memory::buddy::BuddyFrameAllocator;

    const CLONE_SETTLS: u64 = 0x80000;
    const CLONE_PARENT_SETTID: u64 = 0x00100000;
    const CLONE_CHILD_SETTID: u64 = 0x02000000;
    const CLONE_CHILD_CLEARTID: u64 = 0x00200000;

    let parent_lock = CURRENT_PROCESS.lock();
    if let Some(ref parent) = *parent_lock {
        let child_pid = Process::next_id();

        let child_as = match parent.address_space.clone_cow(&mut BuddyFrameAllocator) {
            Some(as_space) => as_space,
            None => return errno::Errno::ENOMEM as u64,
        };

        let mut child_process = Process::new(child_pid, Some(parent.id), child_as);
        {
            let parent_vmas = parent.memory.lock().vmas.clone();
            child_process.memory.lock().vmas = parent_vmas;
        }
        child_process.entry_point = parent.entry_point;
        child_process.files.lock().fd_table = parent.files.lock().fd_table.clone();
        child_process.files.lock().fd_flags = parent.files.lock().fd_flags.clone();
        child_process.files.lock().dir_fds = parent.files.lock().dir_fds.clone();
        *child_process.signal_handlers.lock() = *parent.signal_handlers.lock();
        child_process.clone_credentials_from(parent);
        {
            let p_id = parent.identity.lock();
            let mut c_id = child_process.identity.lock();
            c_id.pgid = p_id.pgid;
            c_id.session = p_id.session;
            c_id.is_group_leader = false;
        }

        if flags & CLONE_CHILD_CLEARTID != 0 && !child_tidptr.is_null() {
            *child_process.clear_child_tid.lock() = child_tidptr as u64;
        }

        if flags & CLONE_CHILD_SETTID != 0 && !child_tidptr.is_null() {
            let val = child_pid as u32;
            if unsafe { user_access::copy_to_user(child_tidptr as *mut u8, core::slice::from_raw_parts(&val as *const u32 as *const u8, 4)) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }

        if flags & CLONE_PARENT_SETTID != 0 && !parent_tid.is_null() {
            let val = child_pid as u32;
            if unsafe { user_access::copy_to_user(parent_tid as *mut u8, core::slice::from_raw_parts(&val as *const u32 as *const u8, 4)) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }

        let child_arc = Arc::new(child_process);

        // Clone thread before registering Ã¢â‚¬â€ a stack-alloc failure must not
        // leave an orphan child in PROCESS_TABLE/children.
        let child_thread = {
            let sched = crate::task::scheduler::this_cpu_sched().lock();
            match sched.current_thread.as_ref() {
                Some(t) => match t.clone_thread(child_arc.clone(), regs_ptr, child_stack) {
                    Some(t) => t,
                    None => return errno::Errno::ENOMEM as u64,
                },
                None => return errno::Errno::EPERM as u64,
            }
        };

        parent.children.lock().push(child_pid);
        crate::task::process::Process::register(child_arc.clone());

        let mut child_thread = child_thread;
        if flags & CLONE_SETTLS != 0 {
            child_thread.fs_base = child_tls;
        }

        crate::task::scheduler::spawn_thread(child_thread);
        return child_pid;
    }

    errno::Errno::EPERM as u64
}

pub fn sys_wait4(pid: i64, status_ptr: *mut i32, options: i32, _rusage: *mut u8) -> u64 {
    const WNOHANG: i32 = 1;
    const WUNTRACED: i32 = 2;
    let parent_id = {
        let lock = CURRENT_PROCESS.lock();
        if let Some(ref p) = *lock { p.id } else { return errno::Errno::ESRCH as u64; }
    };

    let mut child_to_reap = None;
    loop {
        // Find an exited child
        {
            let process_table = crate::task::process::PROCESS_TABLE.lock();
            let parent = match process_table.get(&parent_id) {
                Some(p) => p,
                None => { return 0; }
            };
            let children_pids = parent.children.lock();

            for (index, &child_pid) in children_pids.iter().enumerate() {
                if pid != -1 && child_pid != pid as u64 {
                    continue;
                }
                
                if let Some(child) = process_table.get(&child_pid) {
                    let exit_status = child.exit_code.lock();
                    if let Some(status) = *exit_status {
                        child_to_reap = Some((child_pid, status, index));
                        break;
                    }
                }
            }
        }

        if let Some((child_pid, status, index)) = child_to_reap.take() {
            if !status_ptr.is_null() {
                unsafe { *status_ptr = status; }
            }
            
            {
                let process_table = crate::task::process::PROCESS_TABLE.lock();
                let parent = process_table.get(&parent_id).unwrap();
                parent.children.lock().remove(index);
            }
            crate::task::process::PROCESS_TABLE.lock().remove(&child_pid);
            return child_pid;
        }

        // No child exited yet Ã¢â‚¬â€ check for signals before sleeping
        if check_signal_interrupt() { return errno::Errno::EINTR as u64; }

        // WNOHANG: report nothing to reap instead of blocking.
        if options & WNOHANG != 0 {
            return 0;
        }

        // Block the current thread for one tick instead of spinning; the
        // timer tick re-wakes it and the loop re-scans the child table.
        {
            let mut sched = crate::task::scheduler::this_cpu_sched().lock();
            if let Some(current) = sched.current_thread.as_mut() {
                current.status = crate::task::thread::ThreadStatus::Blocked;
                current.sleep_until = Some(crate::interrupts::get_ticks() + 1);
            }
        }
        crate::task::scheduler::schedule();
    }
}

pub fn sys_getpid() -> u64 {
    use crate::task::process::CURRENT_PROCESS;
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        p.id
    } else {
        0
    }
}

pub fn sys_execve(path_ptr: *const u8, argv_ptr: *const *const u8, _envp_ptr: *const *const u8, _regs_ptr: *mut u64) -> u64 {
    use crate::syscalls::user_access;
    
    // 1. Copy path and argv from user space
    let path = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    // LSM hook: exec permission check
    let subj = crate::security::current_subject();
    if !crate::security::hook_file_perm(&subj, &path, "exec") {
        return errno::Errno::EACCES as u64;
    }

    let mut argv = Vec::new();
    if !argv_ptr.is_null() {
        let mut i = 0;
        loop {
            let mut ptr: *const u8 = core::ptr::null();
            unsafe {
                if user_access::copy_from_user(core::slice::from_raw_parts_mut(&mut ptr as *mut _ as *mut u8, 8), argv_ptr.add(i) as *const u8).is_err() {
                    break;
                }
            }
            if ptr.is_null() { break; }
            if let Ok(s) = unsafe { user_access::read_user_string(ptr, 256) } {
                argv.push(s);
            } else {
                break;
            }
            i += 1;
            if i > 64 { break; } // Limit args
        }
    }

    // 2. Resolve path and check permissions
    let node = match crate::vfs::VFS.lock().resolve_path(&path) {
        Some(n) => n,
        None => return errno::Errno::ENOENT as u64,
    };

    // Require execute permission on the binary
    if !check_node_permission(&node, 1) {
        return errno::Errno::EACCES as u64;
    }

    // Setuid/setgid enforcement
    if let Ok(stat) = node.stat() {
        let mode = stat.st_mode;
        let is_setuid = (mode & 0o4000) != 0;
        let is_setgid = (mode & 0o2000) != 0;
        if is_setuid || is_setgid {
            // LSM: allow setuid execution?
            let subj = crate::security::current_subject();
            if !crate::security::hook_setuid_exec(&subj, &path) {
                return errno::Errno::EACCES as u64;
            }
            let lock = CURRENT_PROCESS.lock();
            if let Some(ref proc) = *lock {
                let mut c = proc.credentials();
                if is_setuid { c.euid = stat.st_uid; c.suid = stat.st_uid; }
                if is_setgid { c.egid = stat.st_gid; c.sgid = stat.st_gid; }
                proc.set_credentials(&c);
            }
        }
    }

    let elf_data = match node.read(usize::MAX) {
        Ok(d) => d,
        Err(_) => return errno::Errno::EIO as u64,
    };

    // 3. Copy fd table and flags from old process. Serialize the whole
    // exec tail across CPUs: concurrent execs (fork children exec'ing on
    // different CPUs) otherwise overwrite the global CURRENT_PROCESS mid-
    // flight, so one exec'd process runs with another's process context
    // (wrong fd table, wrong VMA list -> SIGSEGV on its first heap write).
    static EXEC_LOCK: crate::sync::IrqSafeMutex<()> = crate::sync::IrqSafeMutex::new(());
    let exec_guard = EXEC_LOCK.lock();
    let (old_fd_table, old_fd_flags) = crate::task::process::CURRENT_PROCESS.lock()
        .as_ref().map(|p| (p.files.lock().fd_table.clone(), p.files.lock().fd_flags.clone()))
        .unwrap_or_default();

    // 4. Load ELF into new AddressSpace
    use crate::memory::paging::AddressSpace;
    let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
    let new_as = AddressSpace::new(&mut frame_allocator).expect("Failed to create new AddressSpace");
    
    let process = match crate::task::process::Process::load_elf(&elf_data, new_as) {
        Ok(p) => p,
        Err(_) => return errno::Errno::ENOEXEC as u64,
    };

    // Detect emulation mode based on ELF header
    crate::emulation::set_emulation(&process, &elf_data);
    if *process.emulation.lock() == crate::task::process::EmulationMode::Linux {
        crate::println!("[EMULATION] Running Linux binary: {}", path);
    }

    // Restore fd table and flags, honoring FD_CLOEXEC (internal bit 0x80000,
    // set by F_SETFD / O_CLOEXEC / MFD_CLOEXEC / SFD_CLOEXEC): those fds are
    // closed across exec.
    let mut new_fd_table = old_fd_table;
    for (i, fl) in old_fd_flags.iter().enumerate() {
        if fl & 0x80000 != 0 {
            if let Some(slot) = new_fd_table.get_mut(i) {
                *slot = None;
            }
        }
    }
    process.files.lock().fd_table = new_fd_table;
    process.files.lock().fd_flags = old_fd_flags;

    let entry = process.entry_point;
    let process_arc = Arc::new(process);

    // Activate new address space BEFORE setting up user stack
    // so virt_to_phys can find the freshly-mapped pages.
    unsafe { process_arc.address_space.activate(); }

    // 4. Setup user stack
    let user_rsp = match process_arc.setup_user_stack(&argv) {
        Ok(rsp) => rsp,
        Err(()) => {
            crate::serial_write("[EXEC] OOM: failed to allocate user stack\n");
            return errno::Errno::ENOMEM as u64;
        }
    };

    // 5. Update CURRENT_PROCESS
    {
        let mut cur = CURRENT_PROCESS.lock();
        *cur = Some(process_arc.clone());
    }
    
    // Update current thread's process
    {
        crate::task::scheduler::with_current_thread(|thread| {
            thread.process = Some(process_arc.clone());
        });
    }

    crate::serial_write(&alloc::format!(
        "[EXEC] pid={} path={} elf={} entry={:#x} rsp={:#x}\n",
        process_arc.id, path, elf_data.len(), entry, user_rsp
    ));

    drop(exec_guard);
    unsafe {
        crate::task::thread::jump_to_usermode(entry, user_rsp);
    }
}

pub fn sys_getitimer(which: u64, curr_ptr: *mut u8) -> u64 {
    if which != 0 { return errno::Errno::EINVAL as u64; } // Only ITIMER_REAL
    if curr_ptr.is_null() { return errno::Errno::EFAULT as u64; }

    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let it = process.itimer_real.lock();
    let slice = unsafe {
        core::slice::from_raw_parts(&*it as *const itimerval as *const u8, core::mem::size_of::<itimerval>())
    };
    if unsafe { user_access::copy_to_user(curr_ptr, slice) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

pub fn sys_setitimer(which: u64, new_ptr: *const u8, old_ptr: *mut u8) -> u64 {
    if which != 0 { return errno::Errno::EINVAL as u64; } // Only ITIMER_REAL
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    if !old_ptr.is_null() {
        let it = process.itimer_real.lock();
        let slice = unsafe {
            core::slice::from_raw_parts(&*it as *const itimerval as *const u8, core::mem::size_of::<itimerval>())
        };
        if unsafe { user_access::copy_to_user(old_ptr, slice) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }

    if !new_ptr.is_null() {
        let mut new_it: itimerval = itimerval {
            it_interval: timeval { tv_sec: 0, tv_usec: 0 },
            it_value: timeval { tv_sec: 0, tv_usec: 0 },
        };
        let slice = unsafe {
            core::slice::from_raw_parts_mut(&mut new_it as *mut itimerval as *mut u8, core::mem::size_of::<itimerval>())
        };
        if unsafe { user_access::copy_from_user(slice, new_ptr) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
        if new_it.it_value.tv_sec < 0 || new_it.it_value.tv_usec < 0 {
            return errno::Errno::EINVAL as u64;
        }
        *process.itimer_real.lock() = new_it;
    }
    0
}

pub fn sys_times(buf_ptr: *mut u8) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return 0u64.wrapping_sub(crate::interrupts::get_ticks()),
    };
    let t = tms {
        tms_utime: process.utime.load(core::sync::atomic::Ordering::Relaxed) as i64,
        tms_stime: process.stime.load(core::sync::atomic::Ordering::Relaxed) as i64,
        tms_cutime: process.cutime.load(core::sync::atomic::Ordering::Relaxed) as i64,
        tms_cstime: process.cstime.load(core::sync::atomic::Ordering::Relaxed) as i64,
    };
    if !buf_ptr.is_null() {
        let slice = unsafe {
            core::slice::from_raw_parts(&t as *const tms as *const u8, core::mem::size_of::<tms>())
        };
        if unsafe { user_access::copy_to_user(buf_ptr, slice) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    // Return clock ticks since boot
    crate::interrupts::get_ticks()
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_pause(guest_id: u64) -> u64 {
    let mut hv_lock = crate::hypervisor::HYPERVISOR.lock();
    let hv = match hv_lock.as_mut() {
        Some(hv) => hv,
        None => return errno::Errno::ENODEV as u64,
    };
    match hv.guests.get_mut(&guest_id) {
        Some(guest) => { guest.state = crate::hypervisor::VmState::Paused; 0 }
        None => errno::Errno::ENOENT as u64,
    }
}

