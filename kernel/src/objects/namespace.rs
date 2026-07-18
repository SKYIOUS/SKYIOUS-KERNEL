use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use super::KernelObject;
use spin::Mutex;
use lazy_static::lazy_static;

/// A directory in the global object namespace.
pub struct ObjectDirectory {
    entries: BTreeMap<String, Arc<dyn KernelObject>>,
    subdirs: BTreeMap<String, ObjectDirectory>,
}

impl ObjectDirectory {
    pub fn new() -> Self {
        ObjectDirectory { entries: BTreeMap::new(), subdirs: BTreeMap::new() }
    }

    pub fn insert(&mut self, name: &str, obj: Arc<dyn KernelObject>) {
        self.entries.insert(String::from(name), obj);
    }

    pub fn lookup(&self, path: &str) -> Option<&Arc<dyn KernelObject>> {
        let path = path.trim_matches('/');
        if path.is_empty() { return None; }
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop()?;
        let mut dir = self;
        for part in parts {
            dir = dir.subdirs.get(part)?;
        }
        dir.entries.get(name)
    }

    pub fn lookup_mut(&mut self, path: &str) -> Option<&mut Arc<dyn KernelObject>> {
        let path = path.trim_matches('/');
        if path.is_empty() { return None; }
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop()?;
        let mut dir = self;
        for part in parts {
            dir = dir.subdirs.get_mut(part)?;
        }
        dir.entries.get_mut(name)
    }

    pub fn remove(&mut self, path: &str) -> Option<Arc<dyn KernelObject>> {
        let path = path.trim_matches('/');
        if path.is_empty() { return None; }
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop()?;
        let mut dir = self;
        for part in parts {
            dir = dir.subdirs.get_mut(part)?;
        }
        dir.entries.remove(name)
    }

    pub fn mkdir(&mut self, path: &str) -> bool {
        if path.is_empty() || path == "/" { return false; }
        let path = path.trim_matches('/');
        let parts: Vec<&str> = path.split('/').collect();
        self.mkdir_slice(&parts)
    }

    fn mkdir_slice(&mut self, parts: &[&str]) -> bool {
        let name = match parts.first() {
            None => return false,
            Some(&n) => n,
        };
        if parts.len() == 1 {
            if self.subdirs.contains_key(name) { return false; }
            self.subdirs.insert(String::from(name), ObjectDirectory::new());
            true
        } else {
            self.subdirs.get_mut(name)
                .is_some_and(|d| d.mkdir_slice(&parts[1..]))
        }
    }
}

/// The global kernel object namespace, rooted at `\`.
pub struct ObjectNamespace {
    root: ObjectDirectory,
}

impl ObjectNamespace {
    pub fn new() -> Self {
        ObjectNamespace { root: ObjectDirectory::new() }
    }

    pub fn root(&mut self) -> &mut ObjectDirectory {
        &mut self.root
    }

    pub fn lookup(&self, path: &str) -> Option<&Arc<dyn KernelObject>> {
        self.root.lookup(path)
    }

    pub fn insert(&mut self, path: &str, obj: Arc<dyn KernelObject>) {
        let path = path.trim_matches('/');
        if path.is_empty() { return; }
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop().unwrap();
        let mut dir = &mut self.root;
        for part in parts {
            if !dir.subdirs.contains_key(part) {
                dir.subdirs.insert(String::from(part), ObjectDirectory::new());
            }
            dir = dir.subdirs.get_mut(part).unwrap();
        }
        dir.insert(name, obj);
    }

    pub fn remove(&mut self, path: &str) -> Option<Arc<dyn KernelObject>> {
        self.root.remove(path)
    }
}

lazy_static! {
    /// Global singleton: the kernel object namespace.
    pub static ref OBJECT_NAMESPACE: Mutex<ObjectNamespace> = Mutex::new(ObjectNamespace::new());
}

pub fn resolve_object(path: &str) -> Option<Arc<dyn KernelObject>> {
    OBJECT_NAMESPACE.lock().lookup(path).cloned()
}

pub fn register_object(name: &str, object: Arc<dyn KernelObject>) {
    let path = alloc::format!("System/{}", name);
    OBJECT_NAMESPACE.lock().insert(&path, object);
}

pub fn audit_by_pid(_pid: u64) -> alloc::vec::Vec<(alloc::string::String, super::ObjectTypeId)> {
    alloc::vec::Vec::new()
}

pub fn init() {
    OBJECT_NAMESPACE.lock().root().mkdir("Device");
    OBJECT_NAMESPACE.lock().root().mkdir("Process");
    OBJECT_NAMESPACE.lock().root().mkdir("Tmp");
    OBJECT_NAMESPACE.lock().root().mkdir("System");
}
