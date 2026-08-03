//! Boot state machine types and transition validation.

pub mod logger;
pub mod state;

use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::IrqSafeMutex as Mutex;

static BOOT_TRACE: Mutex<Option<Vec<BootEvent>>> = Mutex::new(None);
static BOOT_INIT_PATHS: Mutex<Option<Vec<String>>> = Mutex::new(None);

pub fn store_trace(trace: Vec<BootEvent>, init_paths: Vec<String>) {
    *BOOT_TRACE.lock() = Some(trace);
    *BOOT_INIT_PATHS.lock() = Some(init_paths);
}

pub fn with_trace<F, R>(f: F) -> R
where
    F: FnOnce(Option<&Vec<BootEvent>>, Option<&Vec<String>>) -> R,
{
    let trace = BOOT_TRACE.lock();
    let paths = BOOT_INIT_PATHS.lock();
    f(trace.as_ref(), paths.as_ref())
}

/// Phases of the boot state machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BootState {
    InitKernel,
    LocateInit,
    ParseElf,
    CreateAddressSpace,
    MapStack,
    CreatePid1,
    SetupConsole,
    EnterUserspace,
    Running,
    Failed,
}

impl BootState {
    /// Valid transitions — any other pair is a programming error.
    pub fn valid_next(&self) -> &[BootState] {
        match self {
            BootState::InitKernel => &[BootState::LocateInit],
            BootState::LocateInit => &[BootState::ParseElf, BootState::Failed],
            BootState::ParseElf => &[BootState::CreateAddressSpace, BootState::Failed],
            BootState::CreateAddressSpace => &[BootState::MapStack, BootState::Failed],
            BootState::MapStack => &[BootState::CreatePid1, BootState::Failed],
            BootState::CreatePid1 => &[BootState::SetupConsole, BootState::Failed],
            BootState::SetupConsole => &[BootState::EnterUserspace, BootState::Running],
            BootState::EnterUserspace => &[BootState::Running, BootState::Failed],
            BootState::Running => &[],
            BootState::Failed => &[],
        }
    }
}

/// Fatal boot errors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BootError {
    InitNotFound,
    InvalidElf,
    AddressSpaceCreationFailed,
    StackAllocationFailed,
    ConsoleUnavailable,
    UserspaceEntryFailed,
}

/// Recoverable boot warnings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BootWarning {
    ConsoleUnavailable,
    EntropySourceMissing,
}

/// Events recorded in the boot trace.
#[derive(Debug, Clone)]
pub enum BootEvent {
    Enter(BootState),
    Exit(BootState),
    Warning(BootWarning),
    Error(BootError),
}

/// Persistent data live across the entire boot lifetime.
pub struct BootContext {
    pub trace: Vec<BootEvent>,
    pub init_paths_tried: Vec<String>,
    pub elf_data: Option<Vec<u8>>,
    pub boot_start_tick: u64,
}

impl BootContext {
    pub fn new(boot_start_tick: u64) -> Self {
        BootContext {
            trace: Vec::new(),
            init_paths_tried: Vec::new(),
            elf_data: None,
            boot_start_tick,
        }
    }
}

/// Transient objects only needed while launching PID 1.
pub struct BootSession {
    pub entry_point: u64,
    pub user_rsp: u64,
}
