pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_STAT: u64 = 4;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const _SYS_MPROTECT: u64 = 10;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;
pub const SYS_CLONE: u64 = 56;
pub const SYS_FORK: u64 = 57;
pub const SYS_EXECVE: u64 = 59;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT4: u64 = 61;
pub const SYS_RT_SIGACTION: u64 = 13;
pub const SYS_RT_SIGRETURN: u64 = 15;
pub const SYS_NANOSLEEP: u64 = 35;
pub const SYS_GETPID: u64 = 39;
pub const SYS_GETPPID: u64 = 110;

pub const SYS_DUP2: u64 = 33;
pub const SYS_PIPE: u64 = 22;
pub const SYS_UNAME: u64 = 63;

pub const SYS_SOCKET: u64 = 41;
pub const SYS_CONNECT: u64 = 42;
pub const SYS_SENDTO: u64 = 44;
pub const SYS_RECVFROM: u64 = 45;
pub const SYS_BIND: u64 = 49;

// GUI Syscalls
pub const SYS_GUI_CREATE_WINDOW: u64 = 100;
pub const SYS_GUI_GET_BUFFER: u64 = 101;
pub const SYS_GUI_FLUSH: u64 = 102;
pub const SYS_GUI_MAP_BUFFER: u64 = 103;

// Audio Syscalls
pub const SYS_BEEP: u64 = 104;

// Additional Filesystem Syscalls
pub const SYS_GETCWD: u64 = 79;
pub const SYS_CHDIR: u64 = 80;
pub const SYS_MKDIR: u64 = 83;
pub const SYS_UNLINK: u64 = 87;
pub const SYS_KILL: u64 = 62;
pub const SYS_RESOLVE: u64 = 200;
pub const SYS_KORLANG: u64 = 201;
pub const SYS_FUTEX: u64 = 202;
pub const SYS_SYSINFO: u64 = 203;
pub const SYS_SCHED_YIELD: u64 = 24;
pub const SYS_SCHED_SETATTR: u64 = 144;
pub const SYS_SCHED_GETATTR: u64 = 145;
pub const SYS_GETDENTS64: u64 = 217;
pub const SYS_IOCTL: u64 = 16;
pub const SYS_CLOCK_GETTIME: u64 = 228;
pub const SYS_MOUNT: u64 = 165;
pub const SYS_UMOUNT2: u64 = 167;
pub const SYS_FCHMOD: u64 = 91;
pub const SYS_FCHOWN: u64 = 93;
pub const SYS_VAHIAI: u64 = 300;
pub const SYS_SYMLINK: u64 = 88;
pub const SYS_READLINK: u64 = 89;
pub const SYS_ARCH_PRCTL: u64 = 158;
