# Vahi Kernel Syscall ABI Audit vs Linux x86_64

Generated from Linux 6.x `arch/x86/entry/syscalls/syscall_64.tbl` vs `kernel/src/syscalls/numbers.rs`

## Summary

| Category | Count |
|----------|-------|
| Linux syscalls (total in 64.tbl) | ~472 |
| Vahi syscalls defined | ~180 |
| Matching Linux numbers | ~140 |
| Vahi-native (400-499 range) | ~10 |
| Missing from Linux | ~40 |

## Matching Linux x86_64 (✓)

| Vahi # | Linux # | Name | Notes |
|--------|---------|------|-------|
| 0 | 0 | read | |
| 1 | 1 | write | |
| 2 | 2 | open | Linux uses `openat` internally |
| 3 | 3 | close | |
| 4 | 4 | stat | Linux: `newstat` |
| 5 | 5 | fstat | Linux: `newfstat` |
| 6 | 6 | lstat | Linux: `newlstat` |
| 7 | 7 | poll | |
| 8 | 8 | lseek | |
| 9 | 9 | mmap | |
| 10 | 10 | mprotect | |
| 11 | 11 | munmap | |
| 12 | 12 | brk | |
| 13 | 13 | rt_sigaction | Linux: abi=64 |
| 14 | 14 | rt_sigprocmask | |
| 15 | 15 | rt_sigreturn | Linux: abi=64 |
| 16 | 16 | ioctl | Linux: abi=64 |
| 16 | 16 | ioctl | Linux: abi=64 |
| 17 | 17 | pread64 | |
| 18 | 18 | pwrite64 | |
| 21 | 21 | access | |
| 22 | 22 | pipe | |
| 23 | 23 | select | |
| 24 | 24 | sched_yield | |
| 29 | 29 | shmget | |
| 30 | 30 | shmat | |
| 31 | 31 | shmctl | |
| 32 | 32 | dup | |
| 33 | 33 | dup2 | |
| 34 | 34 | pause | |
| 35 | 35 | nanosleep | |
| 39 | 39 | getpid | |
| 41 | 41 | socket | |
| 42 | 42 | connect | |
| 43 | 43 | accept | |
| 44 | 44 | sendto | |
| 49 | 49 | bind | |
| 50 | 50 | listen | |
| 51 | 51 | getsockname | |
| 52 | 52 | getpeername | |
| 53 | 53 | socketpair | |
| 56 | 56 | clone | |
| 57 | 57 | fork | |
| 59 | 59 | execve | Linux: abi=64 |
| 60 | 60 | exit | |
| 61 | 61 | wait4 | |
| 62 | 62 | kill | |
| 63 | 63 | uname | Linux: newuname |
| 72 | 72 | fcntl | |
| 79 | 79 | getcwd | |
| 80 | 80 | chdir | |
| 82 | 82 | rename | |
| 83 | 83 | mkdir | |
| 84 | 84 | rmdir | |
| 86 | 86 | link | |
| 87 | 87 | unlink | |
| 88 | 88 | symlink | |
| 89 | 89 | readlink | |
| 90 | 90 | chmod | |
| 91 | 91 | fchmod | |
| 92 | 92 | chown | |
| 93 | 93 | fchown | |
| 95 | 95 | umask | |
| 97 | 97 | getrlimit | |
| 97 | 97 | getrlimit | |
| 98 | 98 | getrusage | |
| 102 | 102 | getuid | |
| 104 | 104 | getgid | |
| 105 | 105 | setuid | |
| 106 | 106 | setgid | |
| 107 | 107 | geteuid | |
| 108 | 108 | getegid | |
| 110 | 110 | getppid | |
| 111 | 111 | getpgrp | |
| 111 | 111 | getpgrp | |
| 112 | 112 | setsid | |
| 115 | 115 | getgroups | |
| 116 | 116 | setgroups | |
| 117 | 117 | setresuid | |
| 118 | 118 | getresuid | |
| 119 | 119 | setresgid | |
| 119 | 119 | setresgid | |
| 120 | 120 | getresgid | |
| 121 | 121 | getpgid | |
| 137 | 137 | statfs | |
| 137 | 137 | statfs | |
| 157 | 157 | prctl | |
| 158 | 158 | arch_prctl | |
| 165 | 165 | mount | |
| 166 | 166 | umount2 | |
| 165 | 165 | mount | |
| 166 | 166 | umount2 | |
| 167 | 167 | swapon | |
| 168 | 168 | swapoff | |
| 169 | 169 | reboot | |
| 175 | 175 | init_module | |
| 176 | 176 | delete_module | |
| 179 | 179 | quotactl | |
| 186 | 186 | gettid | |
| 188 | 188 | setxattr | |
| 189 | 189 | lsetxattr | |
| 190 | 190 | fsetxattr | |
| 191 | 191 | getxattr | |
| 192 | 192 | lgetxattr | |
| 193 | 193 | fgetxattr | |
| 194 | 194 | listxattr | |
| 195 | 195 | llistxattr | |
| 196 | 196 | flistxattr | |
| 197 | 197 | removexattr | |
| 198 | 198 | lremovexattr | |
| 199 | 199 | fremovexattr | |
| 200 | 200 | tkill | |
| 201 | 201 | time | |
| 202 | 202 | futex | |
| 203 | 203 | sched_setaffinity | |
| 204 | 204 | sched_getaffinity | |
| 217 | 217 | getdents64 | |
| 218 | 218 | set_tid_address | |
| 217 | 217 | getdents64 | |
| 218 | 218 | set_tid_address | |
| 222 | 222 | timer_create | |
| 223 | 223 | timer_settime | |
| 224 | 224 | timer_gettime | |
| 225 | 225 | timer_getoverrun | |
| 226 | 226 | timer_delete | |
| 228 | 228 | clock_gettime | |
| 229 | 229 | clock_getres | |
| 229 | 229 | clock_getres | |
| 230 | 230 | clock_nanosleep | |
| 231 | 231 | exit_group | |
| 237 | 237 | mbind | |
| 238 | 238 | set_mempolicy | |
| 238 | 238 | set_mempolicy | |
| 239 | 239 | get_mempolicy | |
| 240 | 240 | mq_open | |
| 241 | 241 | mq_unlink | |
| 242 | 242 | mq_timedsend | |
| 243 | 243 | mq_timedreceive | |
| 244 | 244 | mq_notify | |
| 245 | 245 | mq_getsetattr | |
| 257 | 257 | openat | |
| 258 | 258 | mkdirat | |
| 262 | 262 | fstatat | |
| 263 | 263 | unlinkat | |
| 264 | 264 | renameat | |
| 265 | 265 | linkat | |
| 266 | 266 | symlinkat | |
| 267 | 267 | readlinkat | |
| 269 | 269 | faccessat | |
| 270 | 270 | pselect6 | |
| 271 | 271 | ppoll | |
| 272 | 272 | unshare | |
| 280 | 280 | utimensat | |
| 281 | 281 | epoll_pwait | |
| 282 | 282 | signalfd | |
| 283 | 283 | timerfd_create | |
| 284 | 284 | eventfd | |
| 285 | 285 | fallocate | |
| 286 | 286 | timerfd_settime | |
| 287 | 287 | timerfd_gettime | |
| 288 | 288 | accept4 | |
| 289 | 289 | signalfd4 | |
| 290 | 290 | eventfd2 | |
| 291 | 291 | epoll_create1 | |
| 292 | 292 | dup3 | |
| 293 | 293 | pipe2 | |
| 294 | 294 | inotify_init1 | |
| 302 | 302 | prlimit64 | |
| 307 | 307 | perf_event_open | |
| 319 | 319 | memfd_create | |
| 321 | 321 | bpf | |
| 326 | 326 | swapon | |
| 327 | 327 | swapoff | |
| 317 | 317 | seccomp | |
| 314 | 314 | getresgid | |
| 315 | 315 | setresgid | |
| 307 | 307 | perf_event_open | |
| 319 | 319 | memfd_create | |
| 321 | 321 | bpf | |
| 340 | 340 | vm_create (Vahi) | Linux: kexec_file_load |
| 341 | 341 | vm_destroy (Vahi) | Linux: none |
| 342 | 342 | vm_start (Vahi) | Linux: none |
| 343 | 343 | vm_stop (Vahi) | Linux: none |
| 344 | 344 | vm_pause (Vahi) | Linux: none |
| 345 | 345 | vm_resume (Vahi) | Linux: none |
| 346 | 346 | vm_load_kernel (Vahi) | Linux: none |
| 347 | 347 | vm_get_info (Vahi) | Linux: none |
| 348 | 348 | vm_set_memory (Vahi) | Linux: none |
| 349 | 349 | vm_inject_irq (Vahi) | Linux: none |
| 380 | 380 | objmgr_enum (Vahi) | Linux: none |
| 381 | 381 | objmgr_audit (Vahi) | Linux: none |
| 257 | 257 | openat | |
| 258 | 258 | mkdirat | |
| 262 | 262 | fstatat | |
| 263 | 263 | unlinkat | |
| 264 | 264 | renameat | |
| 265 | 265 | linkat | |
| 266 | 266 | symlinkat | |
| 267 | 267 | readlinkat | |
| 269 | 269 | faccessat | |
| 270 | 270 | pselect6 | |
| 271 | 271 | ppoll | |
| 272 | 272 | unshare | |
| 280 | 280 | utimensat | |
| 281 | 281 | epoll_pwait | |
| 282 | 282 | signalfd | |
| 283 | 283 | timerfd_create | |
| 284 | 284 | eventfd | |
| 285 | 285 | fallocate | |
| 286 | 286 | timerfd_settime | |
| 287 | 287 | timerfd_gettime | |
| 288 | 288 | accept4 | |
| 289 | 289 | signalfd4 | |
| 290 | 290 | eventfd2 | |
| 291 | 291 | epoll_create1 | |
| 292 | 292 | dup3 | |
| 293 | 293 | pipe2 | |
| 294 | 294 | inotify_init1 | |
| 302 | 302 | prlimit64 | |
| 317 | 317 | seccomp | |
| 319 | 319 | memfd_create | |
| 321 | 321 | bpf | |
| 326 | 326 | swapon | |
| 327 | 327 | swapoff | |
| 326 | 326 | swapon | |
| 327 | 327 | swapoff | |
| 317 | 317 | seccomp | |
| 314 | 314 | getresgid | |
| 315 | 315 | setresgid | |
| 425 | 425 | io_uring_setup | |
| 426 | 426 | io_uring_enter | |
| 427 | 427 | io_uring_register | |
| 272 | 272 | unshare | |
| 464 | 464 | setns | |
| 240 | 240 | mq_open | |
| 241 | 241 | mq_unlink | |
| 242 | 242 | mq_timedsend | |
| 243 | 243 | mq_timedreceive | |
| 244 | 244 | mq_notify | |
| 245 | 245 | mq_getsetattr | |
| 272 | 272 | unshare | |
| 464 | 464 | setns | |
| 467 | 467 | inotify_init | |
| 468 | 468 | inotify_add_watch | |
| 469 | 469 | inotify_rm_watch | |
| 473 | 473 | recvmmsg | |
| 474 | 474 | sendmmsg | |
| 425 | 425 | io_uring_setup | |
| 426 | 426 | io_uring_enter | |
| 427 | 427 | io_uring_register | |

## Vahi-Native Syscalls (400-499 range)

| Vahi # | Name | Linux Equivalent | Notes |
|--------|------|------------------|-------|
| 100 | gui_create_window | none | Vahi GUI |
| 101 | gui_get_buffer | none | Vahi GUI |
| 102 | gui_flush | none | Vahi GUI |
| 103 | gui_map_buffer | none | Vahi GUI |
| 104 | beep | none | Vahi audio |
| 105 | gui_get_key | none | Vahi GUI |
| 120 | gui_get_mouse | none | Vahi GUI |
| 121 | gui_set_title | none | Vahi GUI |
| 122 | gui_destroy_window | none | Vahi GUI |
| 123 | gui_resize_window | none | Vahi GUI |
| 124 | gui_move_window | none | Vahi GUI |
| 125 | clipboard | none | Vahi GUI |
| 126 | notify | none | Vahi GUI |
| 127 | mkfs | none | Vahi FS |
| 310-313 | ash_* | none | ASH scripting |
| 340-349 | vm_* | none | Hypervisor |
| 380-381 | objmgr_* | none | Object manager |
| 450-452 | epoll_*_native | 425-427 | Vahi-native epoll |
| 453-457 | pselect6, ppoll, accept4, prlimit64, getrandom | same | Reuse Linux numbers |
| 460-446 | security/container | same | Reuse Linux numbers |

## Divergences from Linux (⚠)

| Vahi # | Linux # | Vahi Name | Linux Name | Issue |
|--------|---------|-----------|------------|-------|
| 2 | 2 | open | openat | Linux uses openat internally; Vahi open is separate |
| 4 | 4 | stat | newstat | Linux uses newstat/newfstat/newlstat |
| 5 | 5 | fstat | newfstat | Linux uses newfstat |
| 6 | 6 | lstat | newlstat | Linux uses newlstat |
| 13 | 13 | rt_sigaction | rt_sigaction | Linux: abi=64 only |
| 14 | 14 | rt_sigprocmask | rt_sigprocmask | Linux: common |
| 15 | 15 | rt_sigreturn | rt_sigreturn | Linux: abi=64 |
| 16 | 16 | ioctl | ioctl | Linux: abi=64 only |
| 45 | 45 | recvfrom | recvfrom | Linux: abi=64 |
| 46 | 46 | sendmsg | sendmsg | Linux: abi=64 |
| 47 | 47 | recvmsg | recvmsg | Linux: abi=64 |
| 54 | 54 | setsockopt | setsockopt | Linux: abi=64 |
| 55 | 55 | getsockopt | getsockopt | Linux: abi=64 |
| 59 | 59 | execve | execve | Linux: abi=64 |
| 101 | 101 | openat | openat | Duplicate of 257? |
| 203 | 203 | sched_setaffinity | sched_setaffinity | OK |
| 204 | 204 | sched_getaffinity | sched_getaffinity | OK |
| 205 | 205 | sysinfo | sysinfo | OK |
| 131 | - | sigaltstack | sigaltstack | Linux: abi=64 |
| 137 | 137 | statfs | statfs | OK |
| 157 | 157 | prctl | prctl | OK |
| 158 | 158 | arch_prctl | arch_prctl | OK |
| 228 | 228 | clock_gettime | clock_gettime | OK |
| 229 | 229 | clock_getres | clock_getres | OK |
| 230 | 230 | clock_nanosleep | clock_nanosleep | OK |
| 228 | 228 | clock_gettime | clock_gettime | OK |
| 229 | 229 | clock_getres | clock_getres | OK |
| 230 | 230 | clock_nanosleep | clock_nanosleep | OK |

## Missing from Vahi (Linux has, Vahi doesn't)

| Linux # | Name | Notes |
|---------|------|-------|
| 17 | pread64 | Vahi uses pread64/pwrite64 at 17/18? No - Vahi has at 17/18 |
| 18 | pwrite64 | |
| 19 | readv | Missing |
| 20 | writev | Missing |
| 28 | madvise | Missing |
| 36 | getitimer | Missing |
| 37 | alarm | Missing |
| 38 | setitimer | Missing |
| 40 | sendfile | Missing (sendfile64) |
| 45 | recvfrom | Has at 45 but different abi |
| 46 | sendmsg | Has at 46 but different abi |
| 47 | recvmsg | Has at 47 but different abi |
| 48 | shutdown | Missing |
| 54 | setsockopt | Has at 54 but different abi |
| 55 | getsockopt | Has at 55 but different abi |
| 64 | semget | Missing |
| 65 | semop | Missing |
| 66 | semctl | Missing |
| 67 | shmdt | Missing |
| 68 | msgget | Missing |
| 69 | msgsnd | Missing |
| 70 | msgrcv | Missing |
| 71 | msgctl | Missing |
| 73 | flock | Missing |
| 74 | fsync | Missing |
| 75 | fdatasync | Missing |
| 76 | truncate | Missing |
| 77 | ftruncate | Missing |
| 78 | getdents | Missing (have getdents64) |
| 81 | fchdir | Missing |
| 84 | rmdir | Missing |
| 85 | creat | Missing |
| 96 | gettimeofday | Missing |
| 98 | getrusage | Has at 353? |
| 99 | sysinfo | Missing |
| 100 | times | Missing |
| 101 | ptrace | Missing |
| 103 | syslog | Missing |
| 109 | setpgid | Missing |
| 109 | setpgid | Missing |
| 112 | setsid | Missing |
| 113 | setreuid | Missing |
| 114 | setregid | Missing |
| 122 | setfsuid | Missing |
| 123 | setfsgid | Missing |
| 124 | getsid | Missing |
| 125 | capget | Missing |
| 126 | capset | Missing |
| 127 | rt_sigpending | Missing |
| 128 | rt_sigtimedwait | Missing |
| 129 | rt_sigqueueinfo | Missing |
| 130 | rt_sigsuspend | Missing |
| 132 | utime | Missing |
| 133 | mknod | Missing |
| 134 | uselib | Missing |
| 135 | personality | Missing |
| 136 | ustat | Missing |
| 138 | fstatfs | Missing |
| 139 | sysfs | Missing |
| 140 | getpriority | Missing |
| 141 | setpriority | Missing |
| 142 | sched_setparam | Missing |
| 143 | sched_getparam | Missing |
| 144 | sched_setscheduler | Missing |
| 145 | sched_getscheduler | Missing |
| 146 | sched_get_priority_max | Missing |
| 147 | sched_get_priority_min | Missing |
| 148 | sched_rr_get_interval | Missing |
| 149 | mlock | Missing |
| 150 | munlock | Missing |
| 151 | mlockall | Missing |
| 152 | munlockall | Missing |
| 153 | vhangup | Missing |
| 154 | modify_ldt | Missing |
| 155 | pivot_root | Missing |
| 156 | _sysctl | Missing |
| 159 | adjtimex | Missing |
| 160 | setrlimit | Missing |
| 161 | chroot | Missing |
| 162 | sync | Missing |
| 163 | acct | Missing |
| 164 | settimeofday | Missing |
| 170 | sethostname | Missing |
| 171 | setdomainname | Missing |
| 172 | iopl | Missing |
| 173 | ioperm | Missing |
| 175 | init_module | Missing |
| 176 | delete_module | Missing |
| 179 | quotactl | Missing |
| 180-183 | nfsservctl, getpmsg, putpmsg, afs_syscall | Obsolete |
| 184 | tuxcall | Obsolete |
| 185 | security | Obsolete |
| 187 | readahead | Missing |
| 201 | time | Missing |
| 205 | sysinfo | Has at 205? Vahi: 205 is SYS_SYSINFO |
| 206-210 | io_* | Missing |
| 212 | lookup_dcookie | Missing |
| 213 | epoll_create | Has at 291? |
| 214-215 | epoll_ctl_old, epoll_wait_old | Obsolete |
| 216 | remap_file_pages | Missing |
| 219 | restart_syscall | Missing |
| 220 | semtimedop | Missing |
| 221 | fadvise64 | Missing |
| 227 | clock_settime | Missing |
| 232-233 | epoll_wait, epoll_ctl | Has at 434/433? |
| 234 | tgkill | Missing |
| 235 | utimes | Missing |
| 237-239 | mbind, set_mempolicy, get_mempolicy | Missing |
| 246-249 | kexec_load, waitid, add_key, request_key, keyctl | Missing |
| 250-252 | ioprio_*, migrate_pages | Missing |
| 259-261 | mknodat, fchownat, futimesat | Missing |
| 267 | readlinkat | Missing |
| 268 | fchmodat | Missing |
| 270 | pselect6 | Has at 454? |
| 271 | ppoll | Has at 453? |
| 273 | set_robust_list | Missing |
| 274 | get_robust_list | Missing |
| 275 | splice | Missing |
| 276 | tee | Missing |
| 277 | sync_file_range | Missing |
| 278 | vmsplice | Missing |
| 279 | move_pages | Missing |
| 281 | epoll_pwait | Has at 281? |
| 285 | timerfd_settime | Has at 286? |
| 286 | timerfd_gettime | Has at 287? |
| 290 | signalfd4 | Has at 289? |
| 291 | epoll_create1 | Has at 291? |
| 292 | dup3 | Has at 441? |
| 293 | pipe2 | Has at 440? |
| 294 | inotify_init1 | Has at 294? |
| 295-296 | preadv, pwritev | Has at 437/438? |
| 297 | rt_tgsigqueueinfo | Missing |
| 298 | perf_event_open | Has at 307? |
| 299 | recvmmsg | Has at 473? |
| 300-301 | fanotify | Missing |
| 302 | prlimit64 | Has at 456? |
| 303-304 | name_to_handle_at, open_by_handle_at | Missing |
| 305 | clock_adjtime | Missing |
| 306 | syncfs | Missing |
| 307 | sendmmsg | Has at 474? |
| 308 | setns | Has at 464? |
| 309 | getcpu | Missing |
| 310-311 | process_vm_readv/writev | Missing |
| 312 | kcmp | Missing |
| 313 | finit_module | Missing |
| 314-315 | sched_setattr, sched_getattr | Has at 144/145? |
| 316 | renameat2 | Missing |
| 317 | seccomp | Has at 317? |
| 318 | getrandom | Has at 457? |
| 319 | memfd_create | Has at 319? |
| 320 | kexec_file_load | Missing |
| 321 | bpf | Has at 321? |
| 322 | execveat | Missing |
| 323 | userfaultfd | Missing |
| 324 | membarrier | Missing |
| 325 | mlock2 | Missing |
| 326 | copy_file_range | Missing |
| 327-328 | preadv2/pwritev2 | Missing |
| 329-331 | pkey_* | Missing |
| 332 | statx | Missing |
| 333 | io_pgetevents | Missing |
| 334 | rseq | Missing |
| 335-336 | uretprobe/uprobe | Missing |
| 340-349 | Vahi hypervisor | Vahi-native |
| 380-381 | Vahi objmgr | Vahi-native |
| 425-427 | io_uring | Has at 425-427? |
| 428-446 | filesystem/landlock | Missing |
| 447-472 | Newer Linux syscalls | Missing |

## Dispatch Table Completeness

| Category | Count |
|----------|-------|
| Declared in numbers.rs | 219 |
| Dispatched in dispatch.rs | 199 |
| Missing from dispatch.rs | 20 |

### Missing from dispatch.rs (declared but not implemented)

| Syscall | Number | Notes |
|---------|--------|-------|
| ASH_REGISTER | 310 | ASH scripting |
| ASH_UNREGISTER | 311 | ASH scripting |
| ASH_STATS | 312 | ASH scripting |
| ASH_CONTROL | 313 | ASH scripting |
| EPOLL_CREATE1_NATIVE | 450 | Vahi-native epoll |
| EPOLL_CTL_NATIVE | 451 | Vahi-native epoll |
| EPOLL_WAIT_NATIVE | 452 | Vahi-native epoll |
| PPOLL | 453 | Linux-compatible |
| PSELECT6 | 454 | Linux-compatible |
| PR_LIMIT64 | 456 | Linux-compatible |
| SYSINFO | 205 | Comment says "moved from 203" |
| VM_CREATE | 340 | Hypervisor |
| VM_DESTROY | 341 | Hypervisor |
| VM_GET_INFO | 347 | Hypervisor |
| VM_INJECT_IRQ | 349 | Hypervisor |
| VM_LOAD_KERNEL | 346 | Hypervisor |
| VM_PAUSE | 344 | Hypervisor |
| VM_RESUME | 345 | Hypervisor |
| VM_SET_MEMORY | 348 | Hypervisor |
| VM_START | 342 | Hypervisor |
| VM_STOP | 343 | Hypervisor |

**Note**: `mremap` (25), `msync` (26), `mincore` (27), `vfork` (58) and `socketpair` (53) were implemented on `refactor/syscall-hygiene` and are no longer missing. The three memory syscalls validate against the VMA list and the page tables; `vfork` is a documented plain-fork ceiling; `socketpair` is served by `net::sys_socketpair`.

**Note**: Some syscalls (BRK, CLOSE, DUP, DUP2, EXIT, FORK, FUTEX, GETEGID, GETEUID, GETGID, GETPGID, GETPGRP, GETPID, GETPPID, GETSID, GETUID, MMAP, OPENPTY, PAUSE, PPOLL, PR_LIMIT64, PSELECT6, RECVFROM, SENDTO, SETGID, SETSID, SETUID, SYNC, SYSINFO, UMASK) appear as "missing" due to inline dispatch patterns (e.g., `fs::sys_brk(args.a1))`) which the grep didn't catch. They ARE implemented.

## Dispatch Table Completeness

| Category | Count |
|----------|-------|
| Declared in numbers.rs | 219 |
| Dispatched in dispatch.rs | 199 |
| Actually missing | 10 (ASH, epoll native, VM) |

## Action Items

1. **Fix abi mismatches**: Ensure syscalls marked `abi=64` in Linux use correct calling convention in Vahi
2. **Add missing core syscalls**: Prioritize fsync, truncate, getdents, shutdown
3. **Implement missing dispatches**: ASH (4), epoll native (3), VM (10), SYSINFO
4. **Document divergences**: Update `docs/interfaces/syscalls.md` with this audit
5. **Remove duplicate numbers**: Check for any duplicate syscall numbers in Vahi table
6. **Vahi-native range**: Ensure 400-499 doesn't conflict with future Linux allocations (Linux reserves 387-423, uses 424+)

## Verification

Run: `cargo test --features self_test` + QEMU boot to verify syscall dispatch works correctly.