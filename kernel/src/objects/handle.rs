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
    pub audit_id: u64,
    pub create_time: u64,
}

/// Per-process table mapping handle numbers to kernel objects.
pub struct HandleTable {
    table: Vec<Option<HandleEntry>>,
}

static NEXT_HANDLE_AUDIT_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

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
        let audit_id = NEXT_HANDLE_AUDIT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        for (i, slot) in self.table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(HandleEntry { object, access_mask: desired_access, flags, offset: 0, audit_id, create_time: 0 });
                return Ok(i as HandleValue);
            }
        }
        let handle = self.table.len() as HandleValue;
        self.table.push(Some(HandleEntry { object, access_mask: desired_access, flags, offset: 0, audit_id, create_time: 0 }));
        Ok(handle)
    }

    /// Insert an object without security check (for kernel-internal handles).
    pub fn insert_kernel(&mut self, object: Arc<dyn KernelObject>, flags: u64) -> HandleValue {
        let audit_id = NEXT_HANDLE_AUDIT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        for (i, slot) in self.table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(HandleEntry { object, access_mask: 0xFFFF, flags, offset: 0, audit_id, create_time: 0 });
                return i as HandleValue;
            }
        }
        let handle = self.table.len() as HandleValue;
        self.table.push(Some(HandleEntry { object, access_mask: 0xFFFF, flags, offset: 0, audit_id, create_time: 0 }));
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
        let audit_id = NEXT_HANDLE_AUDIT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let new_entry = HandleEntry {
            object: entry.object.clone(),
            access_mask: entry.access_mask,
            flags: entry.flags,
            offset: entry.offset,
            audit_id,
            create_time: 0,
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
        let audit_id = NEXT_HANDLE_AUDIT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let new_entry = HandleEntry {
            object: entry.object.clone(),
            access_mask: entry.access_mask,
            flags: entry.flags,
            offset: entry.offset,
            audit_id,
            create_time: 0,
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

    pub fn audit_trail(&self) -> Vec<(HandleValue, u64)> {
        self.table.iter().enumerate().filter_map(|(i, slot)| {
            slot.as_ref().map(|e| (i as HandleValue, e.audit_id))
        }).collect()
    }

    pub fn enumerate(&self) -> Vec<Arc<dyn KernelObject>> {
        self.table.iter().filter_map(|s| s.as_ref().map(|e| e.object.clone())).collect()
    }

    pub fn find_by_type(&self, type_id: super::ObjectTypeId) -> Vec<HandleValue> {
        self.table.iter().enumerate().filter_map(|(i, slot)| {
            slot.as_ref().and_then(|e| {
                if e.object.header().object_type == type_id { Some(i as HandleValue) } else { None }
            })
        }).collect()
    }

    pub fn handle_count_by_type(&self, type_id: super::ObjectTypeId) -> usize {
        self.table.iter().filter(|slot| {
            slot.as_ref().is_some_and(|e| e.object.header().object_type == type_id)
        }).count()
    }

    pub fn reserve_handle(&mut self) -> HandleValue {
        let idx = self.table.iter().position(|s| s.is_none()).unwrap_or_else(|| {
            let len = self.table.len();
            self.table.push(None);
            len
        });
        idx as HandleValue
    }

    pub fn fill_handle(&mut self, handle: HandleValue, object: Arc<dyn KernelObject>, access_mask: u32, flags: u64, audit_id: u64) {
        let idx = handle as usize;
        if idx >= self.table.len() {
            self.table.resize(idx + 1, None);
        }
        self.table[idx] = Some(HandleEntry { object, access_mask, flags, offset: 0, audit_id, create_time: 0 });
    }

    /// Clone the entire table (for fork).
    pub fn clone_table(&self) -> Vec<Option<HandleEntry>> {
        // ponytail: simple clone, no close-on-exec filtering needed yet
        self.table.clone()
    }
}

impl Clone for HandleEntry {
    fn clone(&self) -> Self {
        HandleEntry {
            object: self.object.clone(),
            access_mask: self.access_mask,
            flags: self.flags,
            offset: self.offset,
            audit_id: self.audit_id,
            create_time: self.create_time,
        }
    }
}
