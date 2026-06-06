use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;
use crate::vfs::{VfsNode, Stat};
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};

/// Generates a unique key for each Pipe instance for wake-on-write matching.
static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

pub struct Pipe {
    buffer: Mutex<VecDeque<u8>>,
    capacity: usize,
    id: u64,
}

pub const PIPE_DEFAULT_CAPACITY: usize = 65536;

impl Pipe {
    pub fn new() -> (Arc<PipeReader>, Arc<PipeWriter>) {
        Self::with_capacity(PIPE_DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> (Arc<PipeReader>, Arc<PipeWriter>) {
        let id = NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed);
        let cap = capacity.next_power_of_two().max(4096);
        let pipe = Arc::new(Pipe {
            buffer: Mutex::new(VecDeque::with_capacity(cap)),
            capacity: cap,
            id,
        });
        (
            Arc::new(PipeReader { pipe: pipe.clone() }),
            Arc::new(PipeWriter { pipe }),
        )
    }
}

pub struct PipeReader {
    pipe: Arc<Pipe>,
}

impl VfsNode for PipeReader {
    fn name(&self) -> String { String::from("pipe_reader") }
    fn is_dir(&self) -> bool { false }

    fn read(&self, max_len: usize) -> Result<Vec<u8>, ()> {
        loop {
            let mut buffer = self.pipe.buffer.lock();
            if !buffer.is_empty() {
                let n = buffer.len().min(max_len);
                let data: Vec<u8> = buffer.drain(..n).collect();
                return Ok(data);
            }
            drop(buffer);
            crate::task::scheduler::block_on_pipe(self.pipe.id);
        }
    }

    fn stat(&self) -> Result<Stat, ()> {
        Ok(Stat {
            st_mode: 0o100 | 0o600,
            st_size: self.pipe.buffer.lock().len() as i64,
            ..Default::default()
        })
    }
}

pub struct PipeWriter {
    pipe: Arc<Pipe>,
}

impl VfsNode for PipeWriter {
    fn name(&self) -> String { String::from("pipe_writer") }
    fn is_dir(&self) -> bool { false }
    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> { Err(()) }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        {
            let mut buffer = self.pipe.buffer.lock();
            let available = self.pipe.capacity - buffer.len();
            let to_write = core::cmp::min(available, data.len());
            buffer.extend(&data[..to_write]);
        }
        // Wake any readers blocked on this pipe
        crate::task::scheduler::wake_pipe(self.pipe.id);
        Ok(())
    }

    fn stat(&self) -> Result<Stat, ()> {
        Ok(Stat {
            st_mode: 0o100 | 0o600,
            st_size: self.pipe.buffer.lock().len() as i64,
            ..Default::default()
        })
    }
}

impl Default for Stat {
    fn default() -> Self {
        Stat {
            st_dev: 0, st_ino: 0, st_mode: 0, st_nlink: 0,
            st_uid: 0, st_gid: 0, st_rdev: 0, st_size: 0,
            st_atime: 0, st_mtime: 0, st_ctime: 0,
        }
    }
}
