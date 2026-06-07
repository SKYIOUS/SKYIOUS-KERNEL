# Networking System Calls

The networking syscalls provide socket and network operations. These are currently planned for Phase 2.

## socket (syscall 79)

```c
int socket(int domain, int type, int protocol);
```

Creates a socket endpoint for communication. Returns a file descriptor on success.

**Domains**: `AF_INET`, `AF_INET6`, `AF_UNIX`, `AF_NETLINK`

**Types**: `SOCK_STREAM` (TCP), `SOCK_DGRAM` (UDP), `SOCK_RAW`, `SOCK_SEQPACKET`

## bind (syscall 81)

```c
int bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
```

Binds a socket to a local address.

## listen (syscall 82)

```c
int listen(int sockfd, int backlog);
```

Marks a socket as passive (listening for incoming connections). `backlog` limits the pending connection queue.

## accept (syscall 83)

```c
int accept(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
```

Accepts an incoming connection on a listening socket. Returns a new file descriptor for the connection.

## connect (syscall 80)

```c
int connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
```

Connects a socket to a remote address.

## sendto / recvfrom (syscalls 84-85)

```c
ssize_t sendto(int sockfd, const void *buf, size_t len, int flags, const struct sockaddr *dest_addr, socklen_t addrlen);
ssize_t recvfrom(int sockfd, void *buf, size_t len, int flags, struct sockaddr *src_addr, socklen_t *addrlen);
```

Send and receive datagrams on connectionless sockets.

## sendmsg / recvmsg (syscalls 86-87)

Advanced message send/receive with scatter-gather I/O and ancillary data support.

## setsockopt / getsockopt (syscalls 89-90)

```c
int setsockopt(int sockfd, int level, int optname, const void *optval, socklen_t optlen);
int getsockopt(int sockfd, int level, int optname, void *optval, socklen_t *optlen);
```

Set and get socket options (SO_* for SOL_SOCKET, IP_* for IPPROTO_IP, TCP_* for IPPROTO_TCP).

## shutdown (syscall 88)

```c
int shutdown(int sockfd, int how);
```

Shuts down part of a full-duplex connection. `how`: SHUT_RD, SHUT_WR, SHUT_RDWR.

## getsockname / getpeername (syscalls 91-92)

```c
int getsockname(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
int getpeername(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
```

Get local and remote socket addresses.
