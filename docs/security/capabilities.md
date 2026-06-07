# Capability-Based Security Model

SkyOS uses a capability-based security model to control access to system resources. This document describes the design and implementation of the capability system.

## Overview

In a capability-based model, an unforgeable token (a capability) serves as both the identifier and the access right to a resource. Possession of a capability grants the holder permission to perform specific operations on the resource. This contrasts with ACL-based systems where access is determined by checking a subject's identity against an access control list. Capabilities cannot be guessed or constructed; they must be received through valid channels such as process creation or inter-process communication.

## Capability Types

SkyOS defines several capability types corresponding to kernel resources:

- **Memory capabilities**: Grant the right to map, read, write, or share physical or virtual memory regions.
- **I/O capabilities**: Grant access to specific I/O ports, MMIO regions, or interrupt vectors.
- **Process capabilities**: Grant the right to create, signal, or observe other processes.
- **Namespace capabilities**: Grant the right to bind or look up names in kernel namespaces.
- **Driver capabilities**: Grant the right to communicate with a specific device driver.

Each capability carries an integer type identifier and a set of permission flags (read, write, execute, delete, and transfer).

## Capability Propagation

Capabilities are propagated through two mechanisms:

1. **Inheritance**: When a process forks, the child receives a copy of the parent's capabilities. The parent may specify a restricted subset using the `proc_spawn` system call's capability mask parameter.
2. **Transfer**: A process may send a capability to another process through a dedicated IPC channel. Capabilities sent this way are removed from the sender's capability table to prevent duplication. Transfer is the only way to gain capabilities beyond what was inherited.

## Kernel Enforcement

The kernel enforces capabilities at every resource access point. When a process invokes a system call that operates on a resource, the kernel looks up the corresponding capability in the process's capability table. If the capability is absent or lacks the required permission flags, the call returns an error. This check is performed in the system call dispatcher before any resource operation begins, ensuring that no operation proceeds without authorization.

## Current Status and Future Work

The core capability infrastructure is implemented, including capability tables, inheritance, and transfer. Future work includes implementing capability revocation, capability-based device access, and integrating capabilities with the driver model to enforce fine-grained hardware access control.
