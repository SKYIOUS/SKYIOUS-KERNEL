pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_STAT: u64 = 4;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MPROTECT: u64 = 10;
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

pub const SYS_DUP: u64 = 32;
pub const SYS_DUP2: u64 = 33;
pub const SYS_ACCESS: u64 = 21;
pub const SYS_FCNTL: u64 = 72;
pub const SYS_PIPE: u64 = 22;
pub const SYS_UNAME: u64 = 63;

pub const SYS_SOCKET: u64 = 41;
pub const SYS_CONNECT: u64 = 42;
pub const SYS_SENDTO: u64 = 44;
pub const SYS_RECVFROM: u64 = 45;
pub const SYS_BIND: u64 = 49;
pub const SYS_LISTEN: u64 = 50;
pub const SYS_ACCEPT: u64 = 43;
pub const SYS_SETSOCKOPT: u64 = 54;
pub const SYS_SOCKETPAIR: u64 = 53;
pub const SYS_GETSOCKOPT: u64 = 55;
pub const SYS_SENDMSG: u64 = 46;
pub const SYS_RECVMSG: u64 = 47;
pub const SYS_GETSOCKNAME: u64 = 51;
pub const SYS_GETPEERNAME: u64 = 52;

// GUI Syscalls
pub const SYS_GUI_CREATE_WINDOW: u64 = 100;
pub const SYS_GUI_GET_BUFFER: u64 = 101;
pub const SYS_GUI_FLUSH: u64 = 102;
pub const SYS_GUI_MAP_BUFFER: u64 = 103;
pub const SYS_GUI_GET_KEY: u64 = 105;

// Audio Syscalls
pub const SYS_BEEP: u64 = 104;

// Additional Filesystem Syscalls
pub const SYS_GETCWD: u64 = 79;
pub const SYS_CHDIR: u64 = 80;
pub const SYS_MKDIR: u64 = 83;
pub const SYS_UNLINK: u64 = 87;
pub const SYS_KILL: u64 = 62;
pub const SYS_RESOLVE: u64 = 200;
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
pub const SYS_CHMOD: u64 = 90;
pub const SYS_FCHMOD: u64 = 91;
pub const SYS_CHOWN: u64 = 92;
pub const SYS_FCHOWN: u64 = 93;
pub const SYS_UMASK: u64 = 95;
pub const SYS_SYMLINK: u64 = 88;
pub const SYS_READLINK: u64 = 89;
pub const SYS_RENAME: u64 = 82;
pub const SYS_ARCH_PRCTL: u64 = 158;
pub const SYS_SELECT: u64 = 23;
pub const SYS_POLL: u64 = 7;
pub const SYS_GETUID: u64 = 301;
pub const SYS_GETGID: u64 = 302;
pub const SYS_SETUID: u64 = 303;
pub const SYS_SETGID: u64 = 304;
pub const SYS_GETEUID: u64 = 305;
pub const SYS_GETEGID: u64 = 306;
pub const SYS_CAPGET: u64 = 307;
pub const SYS_CAPSET: u64 = 308;
pub const SYS_SIGPROCMASK: u64 = 309;
pub const SYS_IO_URING_SETUP: u64 = 425;
pub const SYS_IO_URING_ENTER: u64 = 426;
pub const SYS_BPF: u64 = 321;
pub const SYS_SYNC: u64 = 36;
pub const SYS_REBOOT: u64 = 169;
pub const SYS_DRMCTL: u64 = 400;
pub const SYS_HASH: u64 = 401;
pub const SYS_OPENPTY: u64 = 210;
pub const SYS_STATFS: u64 = 137;
pub const SYS_TRUNCATE: u64 = 76;
pub const SYS_FTRUNCATE: u64 = 77;
pub const SYS_SET_TID_ADDRESS: u64 = 218;
pub const SYS_EXIT_GROUP: u64 = 231;

// GUI Extended Syscalls
pub const SYS_GUI_GET_MOUSE: u64 = 120;
pub const SYS_GUI_SET_TITLE: u64 = 121;
pub const SYS_GUI_DESTROY_WINDOW: u64 = 122;
pub const SYS_GUI_RESIZE_WINDOW: u64 = 123;
pub const SYS_GUI_MOVE_WINDOW: u64 = 124;
pub const SYS_CLIPBOARD: u64 = 125;
pub const SYS_NOTIFY: u64 = 126;
pub const SYS_MKFS: u64 = 127;
// ASH (Application-Specific Safe Handler) Syscalls
#[cfg_attr(not(feature = "ash"), allow(dead_code))]
pub const SYS_ASH_REGISTER: u64 = 310;
#[cfg_attr(not(feature = "ash"), allow(dead_code))]
pub const SYS_ASH_UNREGISTER: u64 = 311;
#[cfg_attr(not(feature = "ash"), allow(dead_code))]
pub const SYS_ASH_STATS: u64 = 312;
#[cfg_attr(not(feature = "ash"), allow(dead_code))]
pub const SYS_ASH_CONTROL: u64 = 313;

// Filesystem completions
pub const SYS_LSTAT: u64 = 6;
pub const SYS_SENDFILE: u64 = 40;
pub const SYS_LINK: u64 = 86;
pub const SYS_UTIMENSAT: u64 = 280;
pub const SYS_FALLOCATE: u64 = 285;

// Supplementary groups
pub const SYS_GETGROUPS: u64 = 115;
pub const SYS_SETGROUPS: u64 = 116;

// Credential syscalls (extending our 3xx range)
pub const SYS_GETRESUID: u64 = 118;
pub const SYS_SETRESUID: u64 = 119;
pub const SYS_GETRESGID: u64 = 314;
pub const SYS_SETRESGID: u64 = 315;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_CREATE: u64 = 340;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_DESTROY: u64 = 341;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_START: u64 = 342;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_STOP: u64 = 343;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_PAUSE: u64 = 344;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_RESUME: u64 = 345;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_LOAD_KERNEL: u64 = 346;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_GET_INFO: u64 = 347;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_SET_MEMORY: u64 = 348;
#[cfg_attr(not(feature = "hypervisor"), allow(dead_code))]
pub const SYS_VM_INJECT_IRQ: u64 = 349;

pub const SYS_OBJMGR_ENUM: u64 = 380;
pub const SYS_OBJMGR_AUDIT: u64 = 381;

// *at syscall variants (Linux x86_64 numbers)
pub const SYS_OPENAT: u64 = 257;
pub const SYS_MKDIRAT: u64 = 258;
pub const SYS_FSTATAT: u64 = 262;
pub const SYS_UNLINKAT: u64 = 263;
pub const SYS_RENAMEAT: u64 = 264;
pub const SYS_LINKAT: u64 = 265;
pub const SYS_SYMLINKAT: u64 = 266;
pub const SYS_READLINKAT: u64 = 267;
pub const SYS_FACCESSAT: u64 = 269;

pub const SYS_SETPGID: u64 = 157;
pub const SYS_GETPGID: u64 = 330;
pub const SYS_GETPGRP: u64 = 111;
pub const SYS_SETSID: u64 = 112;
pub const SYS_GETSID: u64 = 331;
pub const SYS_GETRLIMIT: u64 = 97;
pub const SYS_SETRLIMIT: u64 = 98;
pub const SYS_PRLIMIT64: u64 = 332;

// Signal and timer syscalls
pub const SYS_PAUSE: u64 = 34;
pub const SYS_GETITIMER: u64 = 350;
pub const SYS_SETITIMER: u64 = 351;
pub const SYS_TIMES: u64 = 352;
pub const SYS_SIGALTSTACK: u64 = 131;
pub const SYS_SIGNALFD: u64 = 282;
pub const SYS_SIGNALFD4: u64 = 289;
pub const SYS_EVENTFD: u64 = 284;
pub const SYS_EVENTFD2: u64 = 290;

// POSIX timer syscalls
pub const SYS_TIMER_CREATE: u64 = 222;
pub const SYS_TIMER_SETTIME: u64 = 223;
pub const SYS_TIMER_GETTIME: u64 = 224;
pub const SYS_TIMER_GETOVERRUN: u64 = 225;
pub const SYS_TIMER_DELETE: u64 = 226;

// SysV shared memory
pub const SYS_SHMGET: u64 = 29;
pub const SYS_SHMAT: u64 = 30;
pub const SYS_SHMCTL: u64 = 31;
pub const SYS_SHMDT: u64 = 67;

// memfd_create
pub const SYS_MEMFD_CREATE: u64 = 319;

// Swap syscalls
pub const SYS_SWAPON: u64 = 326;
pub const SYS_SWAPOFF: u64 = 327;
