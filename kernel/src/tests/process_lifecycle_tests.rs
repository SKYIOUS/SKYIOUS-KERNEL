// Process lifecycle tests — process creation defaults, table registration,
// fork parent/child relations, fd-table clone/restore (the exec path), and
// address-space CoW clones. Kernel-internal APIs, not the syscall interface.
//
// Consolidated from 12 overlapping cases into the minimal set of distinct
// assertions: empty-table clone round-trips and Mutex set/verify cycles were
// duplicated scaffolding (sync_tests covers mutex semantics); the fd round
// trip now seeds a real entry so structural preservation is actually proven.

use crate::memory::buddy::BuddyFrameAllocator;
use crate::memory::paging::AddressSpace;
use crate::task::process::{EmulationMode, FileDescriptor, Process, PROCESS_TABLE};
use alloc::sync::Arc;

fn new_proc() -> Result<Arc<Process>, &'static str> {
    let mut fa = BuddyFrameAllocator;
    let aspace = AddressSpace::new(&mut fa).ok_or("AddressSpace::new failed")?;
    Ok(Arc::new(Process::new(Process::next_id(), None, aspace)))
}

// ─── Address Space Tests ──────────────────────────────────────────

fn test_aspace_create_and_clone() -> Result<(), &'static str> {
    let mut fa = BuddyFrameAllocator;
    let a = AddressSpace::new(&mut fa).ok_or("aspace A failed")?;
    let b = a.clone_cow(&mut fa).ok_or("first cow clone failed")?;
    let c = b.clone_cow(&mut fa).ok_or("second cow clone failed")?;
    // Independent page tables: all three coexist and drop cleanly.
    drop(c);
    drop(b);
    drop(a);
    Ok(())
}

// ─── Process Creation Tests ───────────────────────────────────────

fn test_new_defaults() -> Result<(), &'static str> {
    let pid = Process::next_id();
    let mut fa = BuddyFrameAllocator;
    let aspace = AddressSpace::new(&mut fa).ok_or("aspace failed")?;
    let proc = Process::new(pid, None, aspace);

    if proc.id != pid {
        return Err("PID mismatch");
    }
    if proc.parent_id.is_some() {
        return Err("parent_id should be None");
    }
    if !proc.children.lock().is_empty() {
        return Err("new process has children");
    }
    let creds = proc.credentials();
    if creds.uid != 0 || creds.gid != 0 {
        return Err("default creds should be uid=0 gid=0");
    }
    if !proc.files.lock().fd_table.is_empty() {
        return Err("new process should have empty fd table");
    }
    if *proc.emulation.lock() != EmulationMode::Native {
        return Err("new process should be Native emulation");
    }
    Ok(())
}

fn test_register_lookup() -> Result<(), &'static str> {
    let proc = new_proc()?;
    let pid = proc.id;
    Process::register(proc.clone());
    {
        let table = PROCESS_TABLE.lock();
        let found = table.get(&pid).ok_or("registered process not found")?;
        if found.id != pid {
            return Err("PID mismatch in table");
        }
        if table.get(&(pid + 1_000_000)).is_some() {
            return Err("unregistered pid found in table");
        }
    }
    PROCESS_TABLE.lock().remove(&pid);
    Ok(())
}

fn test_fork_cycle() -> Result<(), &'static str> {
    let mut fa = BuddyFrameAllocator;
    let parent_aspace = AddressSpace::new(&mut fa).ok_or("parent aspace failed")?;
    let parent_pid = Process::next_id();
    let parent = Arc::new(Process::new(parent_pid, None, parent_aspace));
    Process::register(parent.clone());

    // Simulate fork: child gets a CoW clone of the parent's address space.
    let child_aspace = parent
        .address_space
        .clone_cow(&mut fa)
        .ok_or("child cow clone failed")?;
    let child_pid = Process::next_id();
    let child = Arc::new(Process::new(child_pid, Some(parent_pid), child_aspace));
    Process::register(child.clone());

    if child.id == parent.id {
        return Err("child PID same as parent");
    }
    if child.parent_id != Some(parent_pid) {
        return Err("child parent_id wrong");
    }
    {
        let table = PROCESS_TABLE.lock();
        if table.get(&parent_pid).is_none() || table.get(&child_pid).is_none() {
            return Err("forked processes not both in table");
        }
    }
    PROCESS_TABLE.lock().remove(&parent_pid);
    PROCESS_TABLE.lock().remove(&child_pid);
    Ok(())
}

// ─── FD Table Tests ───────────────────────────────────────────────

fn test_fd_table_roundtrip() -> Result<(), &'static str> {
    // fd_table/fd_flags are parallel vectors; exec clones them out from under
    // the files lock, drops CLOEXEC slots, and restores. Seed one real entry
    // so the round trip proves structure is preserved, not just emptiness.
    let proc = new_proc()?;
    {
        let mut files = proc.files.lock();
        files.fd_table.push(Some(FileDescriptor::SignalFd(0xABCD)));
        files.fd_flags.push(0);
        if files.fd_table.len() != files.fd_flags.len() {
            return Err("table and flags must stay parallel");
        }
    }

    let (cloned_table, cloned_flags) = {
        let files = proc.files.lock();
        (files.fd_table.clone(), files.fd_flags.clone())
    };
    if cloned_table.len() != cloned_flags.len() {
        return Err("clone broke table/flags parallelism");
    }
    if !matches!(cloned_table[0], Some(FileDescriptor::SignalFd(0xABCD))) {
        return Err("clone lost the fd entry");
    }

    // Restore (exec path): content survives and the original is unchanged.
    {
        let mut files = proc.files.lock();
        files.fd_table = cloned_table;
        files.fd_flags = cloned_flags;
    }
    {
        let files = proc.files.lock();
        if files.fd_table.len() != 1 || files.fd_table[0].is_none() {
            return Err("restore lost the fd entry");
        }
        if files.fd_table.len() != files.fd_flags.len() {
            return Err("restore broke table/flags parallelism");
        }
    }
    Ok(())
}

fn test_fd_out_of_bounds() -> Result<(), &'static str> {
    let proc = new_proc()?;
    let files = proc.files.lock();
    if (999 as usize) < files.fd_table.len() {
        return Err("fd 999 should not be in an empty table");
    }
    Ok(())
}

// ─── Registration ─────────────────────────────────────────────────

pub fn register() {
    crate::selftest::register(
        "address_space:create_and_clone",
        test_aspace_create_and_clone,
    );
    crate::selftest::register("process:new_defaults", test_new_defaults);
    crate::selftest::register("process:register_lookup", test_register_lookup);
    crate::selftest::register("process:fork_cycle", test_fork_cycle);
    crate::selftest::register("process:fd_table_roundtrip", test_fd_table_roundtrip);
    crate::selftest::register("process:fd_out_of_bounds", test_fd_out_of_bounds);
}
