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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

static LSM_ENABLED: AtomicBool = AtomicBool::new(false);

struct LsmRule {
    subject: String,
    object: String,
    class: String,
    perm: String,
    allow: bool,
}

static POLICY: spin::Mutex<Vec<LsmRule>> = spin::Mutex::new(Vec::new());

pub fn load_policy(text: &str) {
    let mut rules = POLICY.lock();
    rules.clear();
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
        rules.push(LsmRule {
            subject: parts[0].into(),
            object: parts[1].into(),
            class: parts[2].into(),
            perm: parts[3].into(),
            allow,
        });
    }
    if !rules.is_empty() {
        LSM_ENABLED.store(true, Ordering::Relaxed);
        crate::println!("LSM: {} rules loaded", rules.len());
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

pub fn hook_capable(subject: &str, cap: &str) -> bool {
    check(subject, "*", "capability", cap)
}

pub fn hook_mount_perm(subject: &str, path: &str) -> bool {
    check(subject, path, "mount", "mount")
}

pub fn current_subject() -> String {
    let lock = crate::task::process::CURRENT_PROCESS.lock();
    lock.as_ref().map_or("kernel".into(), |p| alloc::format!("pid:{}", p.id))
}

pub fn reload_policy() {
    use crate::vfs::VFS;
    let vfs = VFS.lock();
    if let Some(node) = vfs.resolve_path("/etc/lsm_policy") {
        if let Ok(data) = node.read(4096) {
            if let Ok(text) = core::str::from_utf8(&data) {
                load_policy(text);
                return;
            }
        }
    }
}

pub fn init() {
    reload_policy();
}
