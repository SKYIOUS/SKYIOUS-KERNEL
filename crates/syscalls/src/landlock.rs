//! Landlock LSM state types and path-access checks. The syscall handlers
//! (`landlock_create_ruleset` / `landlock_add_rule` / `landlock_restrict_self`)
//! live in `kernel/src/syscalls/landlock.rs`, operating on these types.

extern crate alloc;

use crate::errno::Errno;
use alloc::string::String;
use alloc::vec::Vec;

/// A single path-based rule
#[derive(Clone, Debug)]
pub struct LandlockPathRule {
    /// Path prefix that this rule applies to
    pub path_prefix: String,
    /// Allowed access rights (bitmask of LANDLOCK_ACCESS_FS_*)
    pub allowed_access: u64,
    /// Denied access rights (for handled_access_fs)
    pub handled_access: u64,
}

/// A complete Landlock ruleset
#[derive(Clone, Debug, Default)]
pub struct LandlockRuleset {
    /// All path rules
    pub rules: Vec<LandlockPathRule>,
    /// Access rights this ruleset handles (bitmask)
    pub handled_access_fs: u64,
    /// Whether the ruleset is locked (no more modifications allowed)
    pub locked: bool,
}

impl LandlockRuleset {
    /// Check if a path is allowed for the given access.
    /// Returns true if access is permitted.
    pub fn check_access(&self, path: &str, access: u64) -> bool {
        let requested = access & self.handled_access_fs;
        if requested == 0 {
            return true; // Not handled by this ruleset = allow
        }

        // Find matching rules (longest prefix match)
        let mut best_match: Option<&LandlockPathRule> = None;
        let mut best_len = 0;

        for rule in &self.rules {
            if (path.starts_with(&rule.path_prefix)
                || path == rule.path_prefix.trim_end_matches('/'))
                && rule.path_prefix.len() > best_len
            {
                best_match = Some(rule);
                best_len = rule.path_prefix.len();
            }
        }

        match best_match {
            Some(rule) => {
                // Check if all requested access bits are in the allowed set
                (requested & rule.allowed_access) == requested
            }
            None => {
                // No rule matches — deny by default (Landlock is deny-first)
                requested == 0
            }
        }
    }

    /// Add a rule to the ruleset
    pub fn add_rule(&mut self, path: String, allowed_access: u64) -> Result<(), Errno> {
        if self.locked {
            return Err(Errno::EPERM);
        }

        self.rules.push(LandlockPathRule {
            path_prefix: path,
            allowed_access,
            handled_access: self.handled_access_fs,
        });
        Ok(())
    }

    /// Lock the ruleset (no more modifications)
    pub fn lock(&mut self) {
        self.locked = true;
    }
}

/// Per-process Landlock state
#[derive(Default)]
pub struct LandlockState {
    /// Active rulesets (stacked, most restrictive applies)
    pub rulesets: Vec<LandlockRuleset>,
    /// Whether Landlock is enforced for this process
    pub active: bool,
}

impl LandlockState {
    /// Check if a path is allowed for the given access under all active rulesets.
    /// ALL rulesets must allow access (intersection semantics).
    pub fn check_access(&self, path: &str, access: u64) -> bool {
        if !self.active || self.rulesets.is_empty() {
            return true;
        }

        for ruleset in &self.rulesets {
            if !ruleset.check_access(path, access) {
                return false;
            }
        }
        true
    }
}
