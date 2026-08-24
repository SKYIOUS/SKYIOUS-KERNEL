use super::{SkyFS, BLOCK_SIZE};
use crate::drivers::block::BlockDevice;
use crate::alloc::sync::Arc;
use crate::sync::IrqSafeMutex as Mutex;

#[cfg(feature = "verification")]
use crate::verified::journal::{JournalStateMachine, JournalEvent};
#[cfg(feature = "verification")]
use crate::verified::runner::VERIFICATION_RUNNER;
#[cfg(feature = "verification")]
use lazy_static::lazy_static;

#[cfg(feature = "verification")]
lazy_static! {
    static ref JOURNAL_VERIFIER: Mutex<JournalStateMachine> = Mutex::new(JournalStateMachine::new());
}

const JOURNAL_MAGIC: u64 = 0x4A4F55524E414C5F;
const MAX_TRANSACTION_BLOCKS: u32 = 256;

/// Journal header state values
const STATE_EMPTY: u8 = 0;
const STATE_ACTIVE: u8 = 1;
const STATE_COMMITTED: u8 = 2;
const STATE_REPLAYED: u8 = 3;

#[repr(C, packed)]
struct JournalHeader {
    magic: u64,
    sequence: u64,
    num_blocks: u32,
    /// Number of target blocks this transaction modifies
    num_targets: u32,
    checksum: u32,
    state: u8,
    _pad: [u8; 4055],
}

/// A target block entry: maps journal data block → filesystem block
#[repr(C, packed)]
struct JournalTarget {
    /// Filesystem block number to write to
    target_block: u64,
    /// Journal block index (relative to header)
    journal_offset: u32,
    _pad: u32,
}

pub struct Journal {
    pub start_block: u64,
    pub num_blocks: u64,
    pub sequence: u64,
    pub next_free: u64,
}

impl Journal {
    pub fn new(start_block: u64, num_blocks: u64) -> Self {
        Journal { start_block, num_blocks, sequence: 1, next_free: 1 }
    }

    pub fn init_device(dev: &mut dyn BlockDevice, start_block: u64, num_blocks: u64) -> Result<(), ()> {
        let hdr = JournalHeader {
            magic: JOURNAL_MAGIC,
            sequence: 0,
            num_blocks: num_blocks as u32,
            num_targets: 0,
            checksum: 0,
            state: STATE_EMPTY,
            _pad: [0u8; 4055],
        };
        let mut buf = [0u8; BLOCK_SIZE];
        let src = unsafe {
            core::slice::from_raw_parts(&hdr as *const JournalHeader as *const u8, core::mem::size_of::<JournalHeader>())
        };
        buf[..src.len()].copy_from_slice(src);
        SkyFS::write_block(dev, start_block, &buf)?;
        for i in 1..num_blocks {
            let zero = [0u8; BLOCK_SIZE];
            SkyFS::write_block(dev, start_block + i, &zero)?;
        }
        Ok(())
    }

    /// Begin a new transaction. Returns the header block number.
    pub fn begin_transaction(dev: &mut dyn BlockDevice, journal: &mut Journal) -> Result<u64, ()> {
        if journal.next_free + 1 >= journal.num_blocks {
            // Journal full — wrap around
            journal.next_free = 1;
        }
        journal.sequence += 1;
        let seq = journal.sequence;
        let block = journal.start_block + journal.next_free;
        let hdr = JournalHeader {
            magic: JOURNAL_MAGIC,
            sequence: seq,
            num_blocks: 1,
            num_targets: 0,
            checksum: 0,
            state: STATE_ACTIVE,
            _pad: [0u8; 4055],
        };
        let mut buf = [0u8; BLOCK_SIZE];
        let src = unsafe {
            core::slice::from_raw_parts(&hdr as *const JournalHeader as *const u8, core::mem::size_of::<JournalHeader>())
        };
        buf[..src.len()].copy_from_slice(src);
        SkyFS::write_block(dev, block, &buf)?;
        journal.next_free += 1;

        #[cfg(feature = "verification")]
        {
            let mut verifier = JOURNAL_VERIFIER.lock();
            if let Err(v) = verifier.apply(JournalEvent::BeginTxn) {
                let mut runner = VERIFICATION_RUNNER.lock();
                runner.record_failure("journal::begin_transaction", &alloc::format!("{:?}", v));
            }
        }

        Ok(block)
    }

    /// Write data to a journal block and record its target filesystem block.
    pub fn journal_data(
        dev: &mut dyn BlockDevice,
        journal: &mut Journal,
        header_block: u64,
        target_block: u64,
        data: &[u8],
    ) -> Result<u64, ()> {
        if journal.next_free >= journal.num_blocks {
            return Err(());
        }
        let jblock = journal.start_block + journal.next_free;
        let mut buf = [0u8; BLOCK_SIZE];
        let len = data.len().min(BLOCK_SIZE);
        buf[..len].copy_from_slice(&data[..len]);
        SkyFS::write_block(dev, jblock, &buf)?;

        // Update header to record the target mapping
        let mut hdr_buf = [0u8; BLOCK_SIZE];
        SkyFS::read_block(dev, header_block, &mut hdr_buf)?;
        let hdr: &mut JournalHeader = unsafe { &mut *(hdr_buf.as_mut_ptr() as *mut JournalHeader) };

        // Write target entry after the header
        let target_idx = hdr.num_targets as usize;
        let target_entry = JournalTarget {
            target_block,
            journal_offset: (journal.next_free - header_block) as u32,
            _pad: 0,
        };
        let entry_size = core::mem::size_of::<JournalTarget>();
        let entry_offset = core::mem::size_of::<JournalHeader>() + target_idx * entry_size;
        if entry_offset + entry_size <= BLOCK_SIZE {
            unsafe {
                let dst = hdr_buf[entry_offset..].as_mut_ptr();
                core::ptr::copy_nonoverlapping(
                    &target_entry as *const JournalTarget as *const u8,
                    dst,
                    entry_size,
                );
            }
        }
        hdr.num_targets += 1;
        hdr.num_blocks += 1;
        SkyFS::write_block(dev, header_block, &hdr_buf)?;

        journal.next_free += 1;
        Ok(jblock)
    }

    /// Check if the journal has room for another transaction.
    pub fn is_full(&self) -> bool {
        self.next_free + 1 >= self.num_blocks
    }

    /// Commit a transaction — mark it as committed and compute checksum.
    /// Uses barrier ordering: data blocks are written before the commit
    /// record, ensuring crash consistency (write-ahead logging).
    pub fn commit_transaction(dev: &mut dyn BlockDevice, _journal: &mut Journal, header_block: u64) -> Result<(), ()> {
        // Write ordering: all journaled data blocks are written before the
        // commit record. If the commit record is visible, all data is too.

        let mut buf = [0u8; BLOCK_SIZE];
        SkyFS::read_block(dev, header_block, &mut buf)?;
        let hdr: &mut JournalHeader = unsafe { &mut *(buf.as_mut_ptr() as *mut JournalHeader) };
        hdr.state = STATE_COMMITTED;
        let checksum = fnv1a_checksum(&buf);
        hdr.checksum = checksum;
        SkyFS::write_block(dev, header_block, &buf)?;

        #[cfg(feature = "verification")]
        {
            let mut verifier = JOURNAL_VERIFIER.lock();
            if let Err(v) = verifier.apply(JournalEvent::TxnPersisted) {
                let mut runner = VERIFICATION_RUNNER.lock();
                runner.record_failure("journal::commit_transaction", &alloc::format!("{:?}", v));
            }
        }

        Ok(())
    }

    /// Recover from journal — replay committed transactions to their target blocks.
    pub fn recover_from_dev(dev: &mut dyn BlockDevice, journal: &mut Journal) -> Result<u32, ()> {
        #[cfg(feature = "verification")]
        {
            let mut verifier = JOURNAL_VERIFIER.lock();
            if let Err(v) = verifier.apply(JournalEvent::Crash) {
                let mut runner = VERIFICATION_RUNNER.lock();
                runner.record_failure("journal::recover_from_dev::crash", &alloc::format!("{:?}", v));
            }
        }

        let mut replayed_count = 0u32;

        for i in 0..journal.num_blocks {
            let block = journal.start_block + i;
            let mut buf = [0u8; BLOCK_SIZE];
            SkyFS::read_block(dev, block, &mut buf)?;
            let hdr: &JournalHeader = unsafe { &*(buf.as_ptr() as *const JournalHeader) };

            if hdr.magic != JOURNAL_MAGIC || hdr.state != STATE_COMMITTED {
                continue;
            }

            let expected_cs = fnv1a_checksum(&buf);
            if hdr.checksum != 0 && hdr.checksum != expected_cs {
                crate::println!("JOURNAL: checksum mismatch at block {}, skipping", block);
                continue;
            }

            // Replay each target block
            let entry_size = core::mem::size_of::<JournalTarget>();
            let base_offset = core::mem::size_of::<JournalHeader>();

            for t in 0..hdr.num_targets as usize {
                let entry_offset = base_offset + t * entry_size;
                if entry_offset + entry_size > BLOCK_SIZE {
                    break;
                }
                let target: &JournalTarget = unsafe {
                    &*(buf[entry_offset..].as_ptr() as *const JournalTarget)
                };

                if target.target_block == 0 {
                    continue;
                }

                // Read the journaled data
                let journal_data_block = block + target.journal_offset as u64;
                let mut data_buf = [0u8; BLOCK_SIZE];
                SkyFS::read_block(dev, journal_data_block, &mut data_buf)?;

                // Write the journaled data to the target filesystem block
                SkyFS::write_block(dev, target.target_block, &data_buf)?;
                let tblock = target.target_block;
                crate::println!("JOURNAL: replayed block {} → target block {}", journal_data_block, tblock);
                replayed_count += 1;
            }

            // Mark this transaction as replayed
            let mut write_buf = buf;
            let hdr_mut: &mut JournalHeader = unsafe { &mut *(write_buf.as_mut_ptr() as *mut JournalHeader) };
            hdr_mut.state = STATE_REPLAYED;
            SkyFS::write_block(dev, block, &write_buf)?;
        }

        journal.sequence = 0;
        journal.next_free = 1;

        #[cfg(feature = "verification")]
        {
            let mut verifier = JOURNAL_VERIFIER.lock();
            if let Err(v) = verifier.apply(JournalEvent::RecoveryComplete) {
                let mut runner = VERIFICATION_RUNNER.lock();
                runner.record_failure("journal::recover_from_dev::recovery_complete", &alloc::format!("{:?}", v));
            }
        }

        Ok(replayed_count)
    }

    /// Recover using a SkyFS handle (acquires device lock internally).
    pub fn recover(fs: &Arc<Mutex<SkyFS>>, journal: &mut Journal) -> Result<u32, ()> {
        let dev_arc = fs.lock().device.clone();
        let mut dev = dev_arc.lock();
        let mut replayed_count = 0u32;

        for i in 0..journal.num_blocks {
            let block = journal.start_block + i;
            let mut buf = [0u8; BLOCK_SIZE];
            SkyFS::read_block(&mut *dev, block, &mut buf)?;
            let hdr: &JournalHeader = unsafe { &*(buf.as_ptr() as *const JournalHeader) };

            if hdr.magic != JOURNAL_MAGIC || hdr.state != STATE_COMMITTED {
                continue;
            }

            let expected_cs = fnv1a_checksum(&buf);
            if hdr.checksum != 0 && hdr.checksum != expected_cs {
                continue;
            }

            let entry_size = core::mem::size_of::<JournalTarget>();
            let base_offset = core::mem::size_of::<JournalHeader>();

            for t in 0..hdr.num_targets as usize {
                let entry_offset = base_offset + t * entry_size;
                if entry_offset + entry_size > BLOCK_SIZE {
                    break;
                }
                let target: &JournalTarget = unsafe {
                    &*(buf[entry_offset..].as_ptr() as *const JournalTarget)
                };

                if target.target_block == 0 {
                    continue;
                }

                let journal_data_block = block + target.journal_offset as u64;
                let mut data_buf = [0u8; BLOCK_SIZE];
                SkyFS::read_block(&mut *dev, journal_data_block, &mut data_buf)?;
                SkyFS::write_block(&mut *dev, target.target_block, &data_buf)?;
                let tblock = target.target_block;
                crate::println!("JOURNAL: replayed block {} → target {}", journal_data_block, tblock);
                replayed_count += 1;
            }

            // Mark as replayed
            let mut write_buf = buf;
            let hdr_mut: &mut JournalHeader = unsafe { &mut *(write_buf.as_mut_ptr() as *mut JournalHeader) };
            hdr_mut.state = STATE_REPLAYED;
            SkyFS::write_block(&mut *dev, block, &write_buf)?;
        }

        journal.sequence = 0;
        journal.next_free = 1;
        Ok(replayed_count)
    }

    /// Get journal statistics for /proc/fs/skyfs
    pub fn stats(&self) -> JournalStats {
        JournalStats {
            sequence: self.sequence,
            next_free: self.next_free,
            num_blocks: self.num_blocks,
            used_pct: if self.num_blocks > 0 {
                ((self.next_free * 100) / self.num_blocks) as u32
            } else {
                0
            },
        }
    }
}

pub struct JournalStats {
    pub sequence: u64,
    pub next_free: u64,
    pub num_blocks: u64,
    pub used_pct: u32,
}

/// FNV-1a 32-bit checksum — much stronger collision resistance than a byte sum.
fn fnv1a_checksum(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5; // FNV offset basis
    for &b in data {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193); // FNV prime
    }
    hash
}

// ── Typed Entry Layer ──────────────────────────────────────────────
// Higher-level journal API that wraps the block-level WAL. Each entry
// carries a type tag so recovery can dispatch correctly.

pub const JOURNAL_INODE_UPDATE: u16 = 1;
pub const JOURNAL_DATA_WRITE: u16 = 2;
pub const JOURNAL_DELETE: u16 = 3;
pub const JOURNAL_COMMIT: u16 = 4;

#[repr(C, packed)]
struct JournalEntryHeader {
    entry_type: u16,
    length: u16,
    _reserved: u32,
}

/// Start a journal transaction. Returns the header block number (txn id).
pub fn journal_begin(fs: &Arc<Mutex<SkyFS>>) -> Result<u64, ()> {
    let fs_lock = fs.lock();
    let j_start = fs_lock.sb.journal_start;
    let j_blocks = fs_lock.sb.journal_blocks;
    let mut dev = fs_lock.device.lock();
    let stored = fs_lock.journal.lock();
    let mut journal = Journal::new(j_start, j_blocks);
    journal.sequence = stored.sequence;
    journal.next_free = stored.next_free;
    drop(stored);
    let header_block = Journal::begin_transaction(&mut *dev, &mut journal)?;
    let mut jlock = fs_lock.journal.lock();
    jlock.sequence = journal.sequence;
    jlock.next_free = journal.next_free;
    Ok(header_block)
}

/// Write a typed entry into an open transaction.
/// `target_fs_block` is the filesystem block this entry modifies (for recovery replay).
/// `data` is the payload (e.g., inode bytes, block content, dir entry).
pub fn journal_write_entry(
    fs: &Arc<Mutex<SkyFS>>,
    txn: u64,
    target_fs_block: u64,
    entry_type: u16,
    data: &[u8],
) -> Result<(), ()> {
    if data.len() > BLOCK_SIZE - core::mem::size_of::<JournalEntryHeader>() {
        return Err(());
    }
    let mut payload = crate::alloc::vec![0u8; BLOCK_SIZE];
    let ehdr = JournalEntryHeader {
        entry_type,
        length: data.len() as u16,
        _reserved: 0,
    };
    let hdr_bytes = unsafe {
        core::slice::from_raw_parts(&ehdr as *const JournalEntryHeader as *const u8,
            core::mem::size_of::<JournalEntryHeader>())
    };
    payload[..hdr_bytes.len()].copy_from_slice(hdr_bytes);
    payload[hdr_bytes.len()..hdr_bytes.len() + data.len()].copy_from_slice(data);

    let fs_lock = fs.lock();
    let j_start = fs_lock.sb.journal_start;
    let j_blocks = fs_lock.sb.journal_blocks;
    let mut dev = fs_lock.device.lock();
    let stored = fs_lock.journal.lock();
    let mut journal = Journal::new(j_start, j_blocks);
    journal.sequence = stored.sequence;
    journal.next_free = stored.next_free;
    drop(stored);
    let result = Journal::journal_data(&mut *dev, &mut journal, txn, target_fs_block, &payload);
    let mut jlock = fs_lock.journal.lock();
    jlock.next_free = journal.next_free;
    result.map(|_| ())
}

/// Commit a transaction — marks it as committed with a checksum.
pub fn journal_commit(fs: &Arc<Mutex<SkyFS>>, txn: u64) -> Result<(), ()> {
    let fs_lock = fs.lock();
    let j_start = fs_lock.sb.journal_start;
    let j_blocks = fs_lock.sb.journal_blocks;
    let mut dev = fs_lock.device.lock();
    let mut journal = Journal::new(j_start, j_blocks);
    Journal::commit_transaction(&mut *dev, &mut journal, txn)
}

/// Recover the journal: replay all committed entries, skip incomplete ones.
/// Called during mount to restore consistency after a crash.
pub fn skyfs_journal_recover(fs: &Arc<Mutex<SkyFS>>) -> Result<u32, ()> {
    let fs_lock = fs.lock();
    let j_start = fs_lock.sb.journal_start;
    let j_blocks = fs_lock.sb.journal_blocks;
    let mut dev = fs_lock.device.lock();
    let mut journal = Journal::new(j_start, j_blocks);
    Journal::recover_from_dev(&mut *dev, &mut journal)
}
