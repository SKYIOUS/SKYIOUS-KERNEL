use crate::{Stat, VfsNode};
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use vahi_sync::IrqSafeMutex as Mutex;

/// Generates a unique key for each Pipe instance for wake-on-write matching.
static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

pub struct Pipe {
    buffer: Mutex<VecDeque<u8>>,
    capacity: usize,
    id: u64,
    eof: AtomicBool,
    writers: AtomicU64,
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
            eof: AtomicBool::new(false),
            writers: AtomicU64::new(1),
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
    fn name(&self) -> String {
        String::from("pipe_reader")
    }
    fn is_dir(&self) -> bool {
        false
    }

    fn read(&self, max_len: usize) -> Result<Vec<u8>, ()> {
        // Bounded spin: drop the lock between checks so the writer can proceed.
        // On each idle iteration, halt the CPU until the next interrupt.
        let mut attempts = 0u32;
        loop {
            let mut buffer = self.pipe.buffer.lock();
            if !buffer.is_empty() {
                let n = buffer.len().min(max_len);
                let data: Vec<u8> = buffer.drain(..n).collect();
                return Ok(data);
            }
            if self.pipe.eof.load(Ordering::Relaxed) {
                return Ok(Vec::new());
            }
            drop(buffer);
            if vahi_syscalls::signal::has_pending_signal() {
                return Err(());
            }
            if attempts >= 4096 {
                return Err(());
            }
            attempts += 1;
            #[cfg(target_arch = "x86_64")]
            // SAFETY: HLT halts the CPU until the next interrupt arrives.
            // This is safe in a polling loop — the interrupt controller will
            // wake us, and we re-check the condition before acting.
            unsafe {
                core::arch::asm!("hlt");
            }
            #[cfg(not(target_arch = "x86_64"))]
            core::hint::spin_loop();
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
    fn name(&self) -> String {
        String::from("pipe_writer")
    }
    fn is_dir(&self) -> bool {
        false
    }
    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        Err(())
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        {
            let mut buffer = self.pipe.buffer.lock();
            let available = self.pipe.capacity - buffer.len();
            if available == 0 {
                return Err(());
            }
            let to_write = core::cmp::min(available, data.len());
            buffer.extend(&data[..to_write]);
        }
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

impl Drop for PipeWriter {
    fn drop(&mut self) {
        if self.pipe.writers.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.pipe.eof.store(true, Ordering::Relaxed);
        }
    }
}
