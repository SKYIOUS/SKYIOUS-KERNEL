//! Global kernel object namespace.
//!
//! Tree-structured namespace with directories at `/Device`, `/Process`,
//! `/Tmp`, `/System`. Objects are looked up by path (e.g., `/Device/serial0`).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use hashbrown::HashMap;

use crate::KernelObject;

/// A directory in the global object namespace.
pub struct ObjectDirectory {
    entries: HashMap<String, Arc<dyn KernelObject>>,
    subdirs: HashMap<String, ObjectDirectory>,
}

impl ObjectDirectory {
    /// Create an empty directory.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            subdirs: HashMap::new(),
        }
    }
}

impl Default for ObjectDirectory {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectDirectory {
    pub fn insert(&mut self, name: &str, obj: Arc<dyn KernelObject>) {
        self.entries.insert(String::from(name), obj);
    }

    /// Look up an object by relative path.
    pub fn lookup(&self, path: &str) -> Option<&Arc<dyn KernelObject>> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return None;
        }
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop()?;
        let mut dir = self;
        for part in parts {
            dir = dir.subdirs.get(part)?;
        }
        dir.entries.get(name)
    }

    /// Remove an object by relative path.
    pub fn remove(&mut self, path: &str) -> Option<Arc<dyn KernelObject>> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return None;
        }
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop()?;
        let mut dir = self;
        for part in parts {
            dir = dir.subdirs.get_mut(part)?;
        }
        dir.entries.remove(name)
    }

    /// Create a directory at the given path.
    pub fn mkdir(&mut self, path: &str) -> bool {
        if path.is_empty() || path == "/" {
            return false;
        }
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        self.mkdir_slice(&parts)
    }

    fn mkdir_slice(&mut self, parts: &[&str]) -> bool {
        let name = match parts.first() {
            None => return false,
            Some(&n) => n,
        };
        if parts.len() == 1 {
            if self.subdirs.contains_key(name) {
                return false;
            }
            self.subdirs
                .insert(String::from(name), ObjectDirectory::new());
            true
        } else {
            self.subdirs
                .get_mut(name)
                .is_some_and(|d| d.mkdir_slice(&parts[1..]))
        }
    }
}

/// The global kernel object namespace, rooted at `/`.
pub struct ObjectNamespace {
    root: ObjectDirectory,
}

impl ObjectNamespace {
    /// Create an empty namespace.
    pub fn new() -> Self {
        Self {
            root: ObjectDirectory::new(),
        }
    }
}

impl Default for ObjectNamespace {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectNamespace {
    pub fn root(&mut self) -> &mut ObjectDirectory {
        &mut self.root
    }

    /// Look up an object by absolute path.
    pub fn lookup(&self, path: &str) -> Option<&Arc<dyn KernelObject>> {
        self.root.lookup(path)
    }

    /// Insert an object at the given absolute path, creating intermediate directories.
    pub fn insert(&mut self, path: &str, obj: Arc<dyn KernelObject>) {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return;
        }
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop().unwrap();
        let mut dir = &mut self.root;
        for part in parts {
            if !dir.subdirs.contains_key(part) {
                dir.subdirs
                    .insert(String::from(part), ObjectDirectory::new());
            }
            dir = dir.subdirs.get_mut(part).unwrap();
        }
        dir.insert(name, obj);
    }

    /// Remove an object at the given path.
    pub fn remove(&mut self, path: &str) -> Option<Arc<dyn KernelObject>> {
        self.root.remove(path)
    }
}

/// Resolve an object by path from the global namespace.
pub fn resolve_object(path: &str) -> Option<Arc<dyn KernelObject>> {
    use spin::Once;
    static NS: Once<vahi_sync::IrqSafeMutex<ObjectNamespace>> = Once::new();
    NS.call_once(|| vahi_sync::IrqSafeMutex::new(ObjectNamespace::new()))
        .lock()
        .lookup(path)
        .cloned()
}

/// Register an object in the global namespace under `/System/<name>`.
pub fn register_object(name: &str, object: Arc<dyn KernelObject>) {
    use spin::Once;
    static NS: Once<vahi_sync::IrqSafeMutex<ObjectNamespace>> = Once::new();
    let path = alloc::format!("System/{}", name);
    NS.call_once(|| vahi_sync::IrqSafeMutex::new(ObjectNamespace::new()))
        .lock()
        .insert(&path, object);
}

/// Audit trail for a given PID (stub).
pub fn audit_by_pid(_pid: u64) -> Vec<(String, crate::ObjectTypeId)> {
    Vec::new()
}

/// Initialize the global namespace with standard directories.
pub fn init() {
    use spin::Once;
    static NS: Once<vahi_sync::IrqSafeMutex<ObjectNamespace>> = Once::new();
    let mut ns = NS
        .call_once(|| vahi_sync::IrqSafeMutex::new(ObjectNamespace::new()))
        .lock();
    ns.root().mkdir("Device");
    ns.root().mkdir("Process");
    ns.root().mkdir("Tmp");
    ns.root().mkdir("System");
}
