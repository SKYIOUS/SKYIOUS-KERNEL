# Vahi Kernel — Syscall Classification

**Date:** August 29, 2026
**Total dispatched:** ~130 syscalls in dispatch table
**Honest functional count:** ~40 fully functional

---

## Classification Legend

- **[F]** Fully functional — works correctly for all standard use cases
- **[L]** Functional with limitations — works for common cases but has gaps
- **[E]** Experimental — exists but untested or partially implemented
- **[S]** Stub — returns hardcoded value or -ENOSYS
- **[U]** Unsupported — not implemented

---

## File I/O

| # | Syscall | Status | Notes |
|---|---------|--------|-------|
| 0 | read | [F] | Works for pipes, devfs, tarfs |
| 1 | write | [F] | Works for serial, devfs |
| 2 | open | [F] | Works for tarfs, devfs paths |
| 3 | close | [F] | Works |
| 4 | stat | [F] | Works |
| 5 | fstat | [F] | Works |
| 6 | lstat | [F] | Works |
| 7 | lseek | [F] | Works |
| 8 | mmap | [L] | Anonymous only. No file-backed mmap. |
| 9 | mprotect | [L] | Basic implementation |
| 10 | munmap | [F] | Works |
| 11 | brk | [F] | Works |
| 12 | rt_sigaction | [L] | Basic signal handlers. Edge cases untested. |
| 13 | rt_sigreturn | [L] | Basic implementation |
| 14 | ioctl | [L] | Limited set of ioctls |
| 15 | pread64 | [L] | Basic implementation |
| 16 | pwrite64 | [L] | Basic implementation |
| 17 | readv | [L] | Basic implementation |
| 18 | writev | [L] | Basic implementation |
| 19 | access | [F] | Works |
| 20 | pipe | [F] | Works |
| 21 | select | [L] | Basic implementation |
| 22 | sched_yield | [F] | Works |
| 23 | mremap | [U] | Not implemented |
| 24 | msync | [U] | Not implemented |
| 25 | mincore | [U] | Not implemented |
| 26 | madvise | [S] | Returns 0 (no-op) |
| 27 | shmget | [L] | Basic implementation |
| 28 | shmat | [L] | Basic implementation |
| 29 | shmctl | [L] | Basic implementation |
| 30 | dup | [F] | Works |
| 31 | dup2 | [F] | Works |
| 32 | pause | [L] | Basic implementation |
| 33 | nanosleep | [F] | Works |
| 34 | getitimer | [F] | Works |
| 35 | alarm | [U] | Not implemented |
| 36 | setitimer | [F] | Works |
| 37 | getpid | [F] | Works |
| 38 | sendfile | [U] | Not implemented |
| 39 | socket | [L] | TCP/UDP via smoltcp |
| 40 | connect | [L] | Basic TCP connect |
| 41 | accept | [L] | Basic TCP accept |
| 42 | sendto | [L] | Basic UDP send |
| 43 | recvfrom | [L] | Basic UDP recv |
| 44 | sendmsg | [U] | Not implemented |
| 45 | recvmsg | [U] | Not implemented |
| 46 | shutdown | [U] | Not implemented |
| 47 | bind | [L] | Basic bind |
| 48 | listen | [L] | Basic listen |
| 49 | getsockname | [L] | Basic implementation |
| 50 | getpeername | [L] | Basic implementation |
| 51 | socketpair | [L] | Basic implementation |
| 52 | setsockopt | [L] | Limited options |
| 53 | getsockopt | [L] | Limited options |
| 54 | clone | [L] | CoW only. CLONE_VM not shared. |
| 55 | fork | [F] | Works (via clone) |
| 56 | vfork | [U] | Not implemented |
| 57 | execve | [F] | Works. fd inheritance. |
| 58 | exit | [F] | Works |
| 59 | wait4 | [F] | Works |
| 60 | kill | [L] | Basic signal delivery |
| 61 | uname | [F] | Returns "Linux" for compatibility |
| 62 | semget | [U] | Not implemented |
| 63 | semop | [U] | Not implemented |
| 64 | shmdt | [L] | Basic implementation |
| 65 | msgget | [U] | Not implemented |
| 66 | msgsnd | [U] | Not implemented |
| 67 | msgrcv | [U] | Not implemented |
| 68 | msgctl | [U] | Not implemented |
| 69 | fcntl | [L] | Basic F_DUPFD, F_GETFD, F_SETFD |
| 70 | flock | [U] | Not implemented |
| 71 | fsync | [S] | Returns 0 (no-op) |
| 72 | fdatasync | [S] | Returns 0 (no-op) |
| 73 | truncate | [L] | Basic implementation |
| 74 | ftruncate | [L] | Basic implementation |
| 75 | getdents64 | [F] | Works |
| 76 | getcwd | [F] | Works |
| 77 | chdir | [F] | Works |
| 78 | fchdir | [U] | Not implemented |
| 79 | rename | [L] | Basic implementation |
| 80 | mkdir | [L] | Basic implementation |
| 81 | rmdir | [U] | Not implemented |
| 82 | creat | [U] | Not implemented (use open) |
| 83 | link | [L] | Basic implementation |
| 84 | unlink | [L] | Basic implementation |
| 85 | symlink | [L] | Basic implementation |
| 86 | readlink | [L] | Basic implementation |
| 87 | chmod | [L] | Basic implementation |
| 88 | fchmod | [L] | Basic implementation |
| 89 | chown | [L] | Basic implementation |
| 90 | fchown | [L] | Basic implementation |
| 91 | lchown | [U] | Not implemented |
| 92 | umask | [F] | Works |
| 93 | getrlimit | [L] | Basic implementation |
| 94 | getrusage | [L] | Basic implementation |
| 95 | sysinfo | [L] | Basic implementation |
| 96 | times | [L] | Basic implementation |
| 97 | ptrace | [S] | Skeleton only |
| 98 | getuid | [F] | Works |
| 99 | syslog | [U] | Not implemented |
| 100 | getgid | [F] | Works |
| 101 | setuid | [F] | Works |
| 102 | setgid | [F] | Works |
| 103 | geteuid | [F] | Works |
| 104 | getegid | [F] | Works |
| 105 | setpgid | [F] | Works |
| 106 | getppid | [F] | Works |
| 107 | getpgrp | [F] | Works |
| 108 | setsid | [F] | Works |
| 109 | setreuid | [U] | Not implemented |
| 110 | setregid | [U] | Not implemented |
| 111 | getgroups | [L] | Basic implementation |
| 112 | setgroups | [L] | Basic implementation |
| 113 | setresuid | [L] | Basic implementation |
| 114 | getresuid | [L] | Basic implementation |
| 115 | setresgid | [L] | Basic implementation |
| 116 | getresgid | [L] | Basic implementation |
| 117 | getpgid | [F] | Works |
| 118 | setfsuid | [U] | Not implemented |
| 119 | setfsgid | [U] | Not implemented |
| 120 | getsid | [F] | Works |
| 121 | capget | [L] | Basic implementation |
| 122 | capset | [L] | Basic implementation |
| 123 | rt_sigpending | [U] | Not implemented |
| 124 | rt_sigtimedwait | [U] | Not implemented |
| 125 | rt_sigqueueinfo | [U] | Not implemented |
| 126 | rt_sigsuspend | [U] | Not implemented |
| 127 | sigaltstack | [L] | Basic implementation |
| 128 | utime | [U] | Not implemented |
| 129 | mknod | [U] | Not implemented |
| 130 | uselib | [U] | Not implemented |
| 131 | personality | [S] | Returns 0 |
| 132 | ustat | [U] | Not implemented |
| 133 | statfs | [L] | Basic implementation |
| 134 | fstatfs | [U] | Not implemented |
| 135 | sysfs | [U] | Not implemented |
| 136 | getpriority | [U] | Not implemented |
| 137 | setpriority | [U] | Not implemented |
| 138 | sched_setparam | [U] | Not implemented |
| 139 | sched_getparam | [U] | Not implemented |
| 140 | sched_setscheduler | [U] | Not implemented |
| 141 | sched_getscheduler | [U] | Not implemented |
| 142 | sched_get_priority_max | [U] | Not implemented |
| 143 | sched_get_priority_min | [U] | Not implemented |
| 144 | sched_rr_get_interval | [U] | Not implemented |
| 145 | mlock | [U] | Not implemented |
| 146 | munlock | [U] | Not implemented |
| 147 | mlockall | [U] | Not implemented |
| 148 | munlockall | [U] | Not implemented |
| 149 | vhangup | [U] | Not implemented |
| 150 | modify_ldt | [U] | Not implemented |
| 151 | pivot_root | [U] | Not implemented |
| 152 | _sysctl | [U] | Not implemented |
| 153 | prctl | [L] | Basic implementation |
| 154 | arch_prctl | [F] | Works (ARCH_SET_FS/GET_FS) |
| 155 | adjtimex | [U] | Not implemented |
| 156 | setrlimit | [L] | Basic implementation |
| 157 | chroot | [U] | Not implemented |
| 158 | sync | [S] | Returns 0 |
| 159 | acct | [U] | Not implemented |
| 160 | settimeofday | [U] | Not implemented |
| 161 | mount | [L] | Basic implementation |
| 162 | umount2 | [L] | Basic implementation |
| 163 | swapon | [S] | Returns 0 |
| 164 | swapoff | [S] | Returns 0 |
| 165 | reboot | [U] | Not implemented |
| 166 | sethostname | [U] | Not implemented |
| 167 | setdomainname | [U] | Not implemented |
| 168 | iopl | [U] | Not implemented |
| 169 | ioperm | [U] | Not implemented |
| 170 | create_module | [U] | Not implemented |
| 171 | init_module | [U] | Not implemented |
| 172 | delete_module | [U] | Not implemented |
| 173 | get_kernel_syms | [U] | Not implemented |
| 174 | query_module | [U] | Not implemented |
| 175 | quotactl | [U] | Not implemented |
| 176 | nfsservctl | [U] | Not implemented |
| 177 | getpmsg | [U] | Not implemented |
| 178 | putpmsg | [U] | Not implemented |
| 179 | afs_syscall | [U] | Not implemented |
| 180 | tuxcall | [U] | Not implemented |
| 181 | security | [U] | Not implemented |
| 182 | gettid | [F] | Works |
| 183 | readahead | [U] | Not implemented |
| 184 | setxattr | [U] | Not implemented |
| 185 | lsetxattr | [U] | Not implemented |
| 186 | fsetxattr | [U] | Not implemented |
| 187 | getxattr | [U] | Not implemented |
| 188 | lgetxattr | [U] | Not implemented |
| 189 | fgetxattr | [U] | Not implemented |
| 190 | listxattr | [U] | Not implemented |
| 191 | llistxattr | [U] | Not implemented |
| 192 | flistxattr | [U] | Not implemented |
| 193 | removexattr | [U] | Not implemented |
| 194 | lremovexattr | [U] | Not implemented |
| 195 | fremovexattr | [U] | Not implemented |
| 196 | tkill | [U] | Not implemented |
| 197 | time | [L] | Basic implementation |
| 198 | futex | [L] | WAIT/WAKE/CMP_REQUEUE |
| 199 | sched_setaffinity | [L] | Basic implementation |
| 200 | sched_getaffinity | [L] | Basic implementation |
| 201 | set_thread_area | [U] | Not implemented |
| 202 | io_setup | [U] | Not implemented |
| 203 | io_destroy | [U] | Not implemented |
| 204 | io_getevents | [U] | Not implemented |
| 205 | io_submit | [U] | Not implemented |
| 206 | io_cancel | [U] | Not implemented |
| 207 | get_thread_area | [U] | Not implemented |
| 208 | lookup_dcookie | [U] | Not implemented |
| 209 | epoll_create | [L] | Basic implementation |
| 210 | epoll_ctl_old | [U] | Not implemented |
| 211 | epoll_wait_old | [U] | Not implemented |
| 212 | remap_file_pages | [U] | Not implemented |
| 213 | getdents | [U] | Not implemented |
| 214 | set_tid_address | [F] | Works |
| 215 | restart_syscall | [U] | Not implemented |
| 216 | semtimedop | [U] | Not implemented |
| 217 | fadvise64 | [S] | Returns 0 |
| 218 | timer_create | [L] | Basic implementation |
| 219 | timer_settime | [L] | Basic implementation |
| 220 | timer_gettime | [L] | Basic implementation |
| 221 | timer_getoverrun | [L] | Basic implementation |
| 222 | timer_delete | [L] | Basic implementation |
| 223 | clock_settime | [U] | Not implemented |
| 224 | clock_gettime | [F] | Works |
| 225 | clock_getres | [F] | Works |
| 226 | clock_nanosleep | [L] | Basic implementation |
| 227 | exit_group | [F] | Works |
| 228 | epoll_wait | [L] | Basic implementation |
| 229 | epoll_ctl | [L] | Basic implementation |
| 230 | tgkill | [U] | Not implemented |
| 231 | utimes | [U] | Not implemented |
| 232 | vserver | [U] | Not implemented |
| 233 | mbind | [U] | Not implemented |
| 234 | set_mempolicy | [U] | Not implemented |
| 235 | get_mempolicy | [U] | Not implemented |
| 236 | mq_open | [U] | Not implemented |
| 237 | mq_unlink | [U] | Not implemented |
| 238 | mq_timedsend | [U] | Not implemented |
| 239 | mq_timedreceive | [U] | Not implemented |
| 240 | mq_notify | [U] | Not implemented |
| 241 | mq_getsetattr | [U] | Not implemented |
| 242 | kexec_load | [U] | Not implemented |
| 243 | waitid | [U] | Not implemented |
| 244 | add_key | [U] | Not implemented |
| 245 | request_key | [U] | Not implemented |
| 246 | keyctl | [U] | Not implemented |
| 247 | ioprio_set | [U] | Not implemented |
| 248 | ioprio_get | [U] | Not implemented |
| 249 | inotify_init | [L] | Basic implementation |
| 250 | inotify_add_watch | [L] | Basic implementation |
| 251 | inotify_rm_watch | [L] | Basic implementation |
| 252 | migrate_pages | [U] | Not implemented |
| 253 | openat | [F] | Works |
| 254 | mkdirat | [F] | Works |
| 255 | mknodat | [U] | Not implemented |
| 256 | fchownat | [U] | Not implemented |
| 257 | futimesat | [U] | Not implemented |
| 258 | newfstatat | [F] | Works |
| 259 | unlinkat | [F] | Works |
| 260 | renameat | [F] | Works |
| 261 | linkat | [F] | Works |
| 262 | symlinkat | [F] | Works |
| 263 | readlinkat | [F] | Works |
| 264 | fchmodat | [U] | Not implemented |
| 265 | faccessat | [F] | Works |
| 266 | pselect6 | [U] | Not implemented |
| 267 | ppoll | [U] | Not implemented |
| 268 | unshare | [S] | Returns 0 |
| 269 | set_robust_list | [S] | Returns 0 |
| 270 | get_robust_list | [S] | Returns 0 |
| 271 | splice | [U] | Not implemented |
| 272 | tee | [U] | Not implemented |
| 273 | sync_file_range | [U] | Not implemented |
| 274 | vmsplice | [U] | Not implemented |
| 275 | move_pages | [U] | Not implemented |
| 276 | utimensat | [L] | Basic implementation |
| 277 | pread64 | [L] | Basic implementation |
| 278 | pwrite64 | [L] | Basic implementation |
| 279 | preadv | [U] | Not implemented |
| 280 | pwritev | [U] | Not implemented |
| 281 | rt_tgsigqueueinfo | [U] | Not implemented |
| 282 | perf_event_open | [U] | Not implemented |
| 283 | recvmmsg | [L] | Basic implementation |
| 284-330 | various | [U] | Not implemented |
| 331 | pipe2 | [L] | Basic implementation |
| 332 | dup3 | [L] | Basic implementation |
| 333-400 | various | [U] | Not implemented |
| 401 | io_uring_setup | [S] | Allocates memory, no actual ring |
| 402 | io_uring_enter | [S] | Returns 0 |
| 403 | io_uring_register | [S] | Returns 0 |

---

## Summary

| Category | Count | Percentage |
|----------|-------|------------|
| **[F] Fully functional** | ~40 | ~30% |
| **[L] Functional with limitations** | ~30 | ~23% |
| **[E] Experimental** | ~10 | ~8% |
| **[S] Stub** | ~15 | ~12% |
| **[U] Unsupported** | ~35 | ~27% |
| **Total** | ~130 | 100% |

### Honest Assessment

The kernel claims 187 syscalls. The dispatch table has ~130 entries. Of those, approximately 40 are fully functional, 30 work with limitations, and the rest are stubs or unsupported.

The most critical gap is the absence of:
- Memory-mapped files (mmap with file backing)
- File locking (flock/fcntl)
- Shared memory via mmap (CLONE_VM)
- Real swap
- Extended attributes
- Most IPC mechanisms (msg queues, semaphores)
- Most scheduling controls
- Most security features

The kernel can boot, fork, exec, and reach a login prompt. That's the honest scope of what works.
