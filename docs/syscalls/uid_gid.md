# User and Group System Calls

The user/group syscalls manage process credentials and permissions.

## getuid / geteuid (syscalls 41-42)

```c
uid_t getuid(void);
uid_t geteuid(void);
```

Returns the real user ID and effective user ID of the calling process. The real UID identifies the owner of the process; the effective UID is used for permission checks.

## getgid / getegid (syscalls 43-44)

```c
gid_t getgid(void);
gid_t getegid(void);
```

Returns the real group ID and effective group ID of the calling process.

## setuid (syscall 45)

```c
int setuid(uid_t uid);
```

Sets the effective user ID of the calling process. If the caller has `CAP_SETUID`, the real, effective, and saved user IDs are all set. Otherwise, only the effective UID can be set to the real UID or saved set-user-ID.

## setgid (syscall 46)

```c
int setgid(gid_t gid);
```

Sets the effective group ID. Same privilege rules as `setuid`.

## getgroups (syscall 47)

```c
int getgroups(int size, gid_t list[]);
```

Returns the list of supplementary group IDs for the calling process. If `size` is 0, returns the number of groups without modifying `list`.

## setgroups (syscall 48)

```c
int setgroups(size_t size, const gid_t *list);
```

Sets the supplementary group IDs. Requires `CAP_SETGID`.

## Process Credentials

Each process has:
- **Real UID/GID**: The actual owner of the process
- **Effective UID/GID**: Used for access control checks
- **Saved set-user-ID**: Allows swapping between real and effective UID
- **Supplementary groups**: Additional group memberships

## Capabilities

SkyOS uses a capability-based security model alongside traditional UID/GID:

```c
int capget(struct cap_header *header, struct cap_data *data);
int capset(struct cap_header *header, const struct cap_data *data);
```

Capabilities include `CAP_SYS_TIME`, `CAP_NET_RAW`, `CAP_SYS_ADMIN`, etc.
