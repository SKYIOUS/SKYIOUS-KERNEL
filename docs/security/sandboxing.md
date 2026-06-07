# Process Sandboxing and Isolation

SkyOS provides multiple layers of isolation to contain processes and limit the impact of security compromises. This document describes the sandboxing mechanisms currently implemented and those planned.

## Process Isolation

Every process in SkyOS runs in its own address space with hardware-enforced isolation. The kernel uses x86-64 paging structures with per-process page tables, ensuring that no process can access another process's memory without explicit kernel-mediated sharing. The kernel enforces SMEP (Supervisor Mode Execution Prevention) and SMAP (Supervisor Mode Access Prevention) to prevent execution of userspace code in kernel mode and to prevent the kernel from accessing userspace memory without explicit intent.

## Capability Confinement

Each process holds a capability table that defines what resources it may access. The kernel's system call dispatcher checks capability grants before servicing requests. A process can only affect resources for which it holds the corresponding capability. This confinement is enforced even for privileged processes; there is no omnipotent root user in the capability model. The spawn system call accepts a capability mask that allows a parent to create a child with strictly fewer privileges than itself.

## Driver Isolation

Device drivers run in a restricted execution environment. Drivers are sandboxed such that a buggy or malicious driver cannot compromise the entire kernel. The driver framework enforces access control at the driver boundary:

- Drivers receive only the I/O ranges and interrupt vectors they explicitly request.
- Driver-to-driver communication is mediated by the kernel, preventing direct memory sharing.
- Driver memory allocations are tracked and cannot be used to corrupt kernel data structures.
- Hardware DMA is restricted to buffers that the driver has explicitly registered.

## Userspace Sandboxing (Planned)

Future releases will introduce additional userspace sandboxing features:

- **Seccomp-style syscall filtering**: Allow processes to restrict which system calls they may invoke.
- **Namespace isolation**: Process-local views of filesystem mounts, process IDs, and IPC objects.
- **Resource limits**: Per-process caps on memory, CPU time, file descriptors, and I/O bandwidth.
- **Network sandboxing**: Per-process firewall rules controlling network access.

These features build on the existing capability infrastructure and extend the principle of least privilege to every aspect of process execution.

## Audit and Monitoring

The kernel's audit subsystem (in development) will log sandboxing-related events: capability checks, denied system calls, driver boundary violations, and process compartment boundary crossings. These logs are intended for security monitoring and forensic analysis.
