//! Lightweight LSM (Linux Security Module) skeleton.
//!
//! Provides hook points for Mandatory Access Control. When no policy is loaded
//! (the default), all operations pass through — traditional DAC only.
//! Create `/etc/lsm_policy` with rules to activate.
//!
//! Rule format (one per line): `subject:object:class:perm:allow|deny`
//!   subject: binary path or "*"  (currently unused; PID-based matching planned)
//!   object:  resource path or "*"
//!   class:   "file", "dir", "process", "capability", "mount"
//!   perm:    "read", "write", "exec", "kill", "mount", "cap_sys_admin", ...

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use lazy_static::lazy_static;

static LSM_ENABLED: AtomicBool = AtomicBool::new(false);
static LSM_VERSION: AtomicU64 = AtomicU64::new(0);

struct LsmRule {
    subject: String,
    object: String,
    class: String,
    perm: String,
    allow: bool,
}

static POLICY: spin::Mutex<Vec<LsmRule>> = spin::Mutex::new(Vec::new());

// Syscall filter: bitmask of allowed syscalls per process
// 0 = denied, 1 = allowed. Default is all allowed (u64::MAX)
lazy_static! {
    static ref SYSCALL_FILTER: spin::Mutex<hashbrown::HashMap<u64, u64>> = spin::Mutex::new(hashbrown::HashMap::new());
}

pub fn set_syscall_filter(pid: u64, filter_mask: u64) {
    SYSCALL_FILTER.lock().insert(pid, filter_mask);
}

pub fn clear_syscall_filter(pid: u64) {
    SYSCALL_FILTER.lock().remove(&pid);
}

pub fn check_syscall_allowed(syscall_number: u64) -> bool {
    let lock = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(process) = lock.as_ref() {
        let pid = process.id;
        let filters = SYSCALL_FILTER.lock();
        if let Some(&mask) = filters.get(&pid) {
            return (mask & (1u64 << (syscall_number % 64))) != 0;
        }
    }
    true // No filter means all syscalls allowed
}

pub fn load_policy(text: &str) -> bool {
    let mut new_rules = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let parts: Vec<&str> = line.splitn(5, ':').collect();
        if parts.len() < 5 { continue; }
        let allow = match parts[4] {
            "allow" => true,
            "deny" => false,
            _ => continue,
        };
        new_rules.push(LsmRule {
            subject: parts[0].into(),
            object: parts[1].into(),
            class: parts[2].into(),
            perm: parts[3].into(),
            allow,
        });
    }
    if !new_rules.is_empty() {
        let mut rules = POLICY.lock();
        *rules = new_rules;
        LSM_ENABLED.store(true, Ordering::Relaxed);
        LSM_VERSION.fetch_add(1, Ordering::Release);
        crate::println!("LSM: {} rules loaded, version {}", rules.len(), LSM_VERSION.load(Ordering::Acquire));
        true
    } else {
        false
    }
}

fn check(subject: &str, object: &str, class: &str, perm: &str) -> bool {
    if !LSM_ENABLED.load(Ordering::Relaxed) { return true; }
    let mut allowed = true;
    for rule in POLICY.lock().iter() {
        if (rule.subject == "*" || rule.subject == subject)
            && (rule.object == "*" || rule.object == object)
            && rule.class == class
            && rule.perm == perm
        {
            allowed = rule.allow;
        }
    }
    allowed
}

pub fn hook_file_perm(subject: &str, path: &str, perm: &str) -> bool {
    check(subject, path, "file", perm)
}

pub fn hook_file_create(subject: &str, path: &str) -> bool {
    check(subject, path, "file", "create")
}

pub fn hook_file_unlink(subject: &str, path: &str) -> bool {
    check(subject, path, "file", "unlink")
}

pub fn hook_dir_mkdir(subject: &str, path: &str) -> bool {
    check(subject, path, "dir", "create")
}

pub fn hook_setuid_exec(subject: &str, path: &str) -> bool {
    // Allow setuid only if LSM doesn't explicitly deny it
    check(subject, path, "process", "setuid_exec")
}

pub fn hook_socket_create(subject: &str, family: u64) -> bool {
    let fam = match family { 2 => "ipv4", 10 => "ipv6", _ => "raw" };
    check(subject, fam, "socket", "create")
}

pub fn hook_socket_connect(subject: &str, addr: &str) -> bool {
    check(subject, addr, "socket", "connect")
}

pub fn current_subject() -> String {
    let lock = crate::task::process::CURRENT_PROCESS.lock();
    lock.as_ref().map_or("kernel".into(), |p| alloc::format!("pid:{}", p.id))
}

pub fn reload_policy() -> bool {
    use crate::vfs::VFS;
    let vfs = VFS.lock();
    if let Some(node) = vfs.resolve_path("/etc/lsm_policy") {
        if let Ok(data) = node.read(4096) {
            if let Ok(text) = core::str::from_utf8(&data) {
                return load_policy(text);
            }
        }
    }
    false
}

pub fn init() {
    reload_policy();
}
