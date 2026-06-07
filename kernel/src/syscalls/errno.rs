#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum Errno {
    Success = 0,
    EPERM = -1,      /* Operation not permitted */
    ENOENT = -2,     /* No such file or directory */
    ESRCH = -3,      /* No such process */
    EINTR = -4,      /* Interrupted system call */
    EIO = -5,        /* I/O error */
    ENXIO = -6,      /* No such device or address */
    E2BIG = -7,      /* Argument list too long */
    ENOEXEC = -8,    /* Exec format error */
    EBADF = -9,      /* Bad file number */
    ECHILD = -10,    /* No child processes */
    EAGAIN = -11,    /* Try again */
    ENOMEM = -12,    /* Out of memory */
    EACCES = -13,    /* Permission denied */
    EFAULT = -14,    /* Bad address */
    ENOTBLK = -15,   /* Block device required */
    EBUSY = -16,     /* Device or resource busy */
    EEXIST = -17,    /* File exists */
    EXDEV = -18,     /* Cross-device link */
    ENODEV = -19,    /* No such device */
    ENOTTY = -25,    /* Inappropriate ioctl for device */
    ETXTBSY = -26,   /* Text file busy */
    EFBIG = -27,     /* File too large */
    ENOSPC = -28,    /* No space left on device */
    ESPIPE = -29,    /* Illegal seek */
    EROFS = -30,     /* Read-only file system */
    EMLINK = -31,    /* Too many links */
    EPIPE = -32,     /* Broken pipe */
    EDOM = -33,      /* Math argument out of domain of func */
    ERANGE = -34,    /* Math result not representable */
    EINVAL = -22,    /* Invalid argument */
    ENOSYS = -38,    /* Function not implemented */
    ELOOP = -40,     /* Too many levels of symbolic links */
    ENOTDIR = -20,   /* Not a directory */
    EISDIR = -21,    /* Is a directory */
    EAFNOSUPPORT = -97, /* Address family not supported by protocol */
    EADDRINUSE = -98,   /* Address already in use */
    EOPNOTSUPP = -95,   /* Operation not supported on transport endpoint */
    ECONNREFUSED = -111,/* Connection refused */
    EALREADY = -114,    /* Operation already in progress */
}

impl From<Errno> for u64 {
    fn from(errno: Errno) -> Self {
        errno as u64
    }
}
