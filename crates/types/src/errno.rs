//! Errno vocabulary for the vahi kernel crate boundary.

// ─── Errno ──────────────────────────────────────────────────────────────────
// Shared POSIX error codes used across syscall and IPC modules.

/// POSIX errno values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
#[allow(clippy::upper_case_acronyms)]
pub enum Errno {
    Success = 0,
    EPERM = -1,
    ENOENT = -2,
    ENFILE = -23, /* File table overflow */
    ESRCH = -3,
    EINTR = -4,
    EIO = -5,
    ENXIO = -6,
    E2BIG = -7,
    ENOEXEC = -8,
    EBADF = -9,
    ECHILD = -10,
    EAGAIN = -11,
    ENOMEM = -12,
    EACCES = -13,
    EFAULT = -14,
    ENOTBLK = -15,
    EBUSY = -16,
    EEXIST = -17,
    EXDEV = -18,
    ENODEV = -19,
    ENOTTY = -25,
    ETXTBSY = -26,
    EFBIG = -27,
    ENOSPC = -28,
    ESPIPE = -29,
    EROFS = -30,
    EMLINK = -31,
    EPIPE = -32,
    EDOM = -33,
    ERANGE = -34,
    EINVAL = -22,
    ENOSYS = -38,
    ELOOP = -40,
    ENOTDIR = -20,
    EISDIR = -21,
    EAFNOSUPPORT = -97,
    EADDRINUSE = -98,
    EOPNOTSUPP = -95,
    ECONNREFUSED = -111,
    ECONNRESET = -104,
    EALREADY = -114,
    EDESTADDRREQ = -89,
    ENOPROTOOPT = -92,
    ENOTSOCK = -88,
    ENOTCONN = -107,
    EIDRM = -43,
    ECANCELED = -125,
    EOWNERDEAD = -130,
    ENOTRECOVERABLE = -131,
    EMSGSIZE = -90,
}

impl Errno {
    /// Returns `true` if this errno indicates a temporary failure (retry recommended).
    #[must_use]
    pub const fn is_transient(self) -> bool {
        matches!(self, Self::EAGAIN | Self::EINTR | Self::ENOMEM)
    }
}

impl From<Errno> for u64 {
    fn from(e: Errno) -> u64 {
        e as i64 as u64
    }
}

impl core::fmt::Display for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}
