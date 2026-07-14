use alloc::sync::Arc;
use alloc::vec::Vec;
use super::{KernelObject, security, current_credentials};

pub type HandleValue = u64;
pub const INVALID_HANDLE: HandleValue = u64::MAX;

/// Per-handle metadata.
pub struct HandleEntry {
    pub object: Arc<dyn KernelObject>,
    pub access_mask: u32,
    pub flags: u64,
    pub offset: u64,
}

/// Per-process table mapping handle numbers to kernel objects.
pub struct HandleTable {
    table: Vec<Option<HandleEntry>>,
}

impl HandleTable {
    pub fn new() -> Self {
        HandleTable { table: Vec::new() }
    }

    pub fn is_valid(&self, handle: HandleValue) -> bool {
        (handle as usize) < self.table.len() && self.table[handle as usize].is_some()
    }

    pub fn get(&self, handle: HandleValue) -> Option<&HandleEntry> {
        self.table.get(handle as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, handle: HandleValue) -> Option<&mut HandleEntry> {
        self.table.get_mut(handle as usize)?.as_mut()
    }

    /// Insert an object with a bind-time security check.
    pub fn insert(&mut self, object: Arc<dyn KernelObject>, desired_access: u32, flags: u64) -> Result<HandleValue, ()> {
        let cred = current_credentials();
        let sec = object.header().security.lock();
        if !security::access_check(&cred, &sec, desired_access) {
            return Err(());
        }
        drop(sec);
        for (i, slot) in self.table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(HandleEntry { object, access_mask: desired_access, flags, offset: 0 });
                return Ok(i as HandleValue);
            }
        }
        let handle = self.table.len() as HandleValue;
        self.table.push(Some(HandleEntry { object, access_mask: desired_access, flags, offset: 0 }));
        Ok(handle)
    }

    /// Insert an object without security check (for kernel-internal handles).
    pub fn insert_kernel(&mut self, object: Arc<dyn KernelObject>, flags: u64) -> HandleValue {
        for (i, slot) in self.table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(HandleEntry { object, access_mask: 0xFFFF, flags, offset: 0 });
                return i as HandleValue;
            }
        }
        let handle = self.table.len() as HandleValue;
        self.table.push(Some(HandleEntry { object, access_mask: 0xFFFF, flags, offset: 0 }));
        handle
    }

    /// Close a handle, returning a reference to the object for further cleanup.
    pub fn close(&mut self, handle: HandleValue) -> Option<Arc<dyn KernelObject>> {
        let entry = self.table.get_mut(handle as usize)?;
        entry.take().map(|e| { e.object.on_close(); e.object })
    }

    /// Duplicate a handle (dup/dup2).
    pub fn dup(&mut self, old_handle: HandleValue) -> Result<HandleValue, ()> {
        let entry = self.table.get(old_handle as usize).ok_or(())?.as_ref().ok_or(())?;
        let new_entry = HandleEntry {
            object: entry.object.clone(),
            access_mask: entry.access_mask,
            flags: entry.flags,
            offset: entry.offset,
        };
        for (i, slot) in self.table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(new_entry);
                return Ok(i as HandleValue);
            }
        }
        let handle = self.table.len() as HandleValue;
        self.table.push(Some(new_entry));
        Ok(handle)
    }

    /// Duplicate into a specific slot (dup2).
    pub fn dup_into(&mut self, old_handle: HandleValue, new_handle: HandleValue) -> Result<HandleValue, ()> {
        let entry = self.table.get(old_handle as usize).ok_or(())?.as_ref().ok_or(())?;
        let new_entry = HandleEntry {
            object: entry.object.clone(),
            access_mask: entry.access_mask,
            flags: entry.flags,
            offset: entry.offset,
        };
        if new_handle as usize >= self.table.len() {
            self.table.resize(new_handle as usize + 1, None);
        }
        self.table[new_handle as usize] = Some(new_entry);
        Ok(new_handle)
    }

    /// Return the number of currently open handles.
    pub fn count(&self) -> usize {
        self.table.iter().filter(|s| s.is_some()).count()
    }

    /// Clone the entire table (for fork).
    pub fn clone_table(&self) -> Vec<Option<HandleEntry>> {
        // ponytail: simple clone, no close-on-exec filtering needed yet
        self.table.iter().map(|e| e.clone()).collect()
    }
}

impl Clone for HandleEntry {
    fn clone(&self) -> Self {
        HandleEntry {
            object: self.object.clone(),
            access_mask: self.access_mask,
            flags: self.flags,
            offset: self.offset,
        }
    }
}
