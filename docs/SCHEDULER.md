# SkyOS Scheduler Design

This document describes the design of the SkyOS scheduler, covering its architecture, scheduling policy, and concurrency model.

## 1. Architecture

SkyOS uses a hybrid scheduling model:

1.  **Preemptive Multi-tasking:** For kernel `Thread`s, a preemptive, priority-based scheduler is used. Preemption is driven by the Local APIC (LAPIC) timer, which fires every 10ms.
2.  **Cooperative Multi-tasking:** For `async` tasks within a thread (e.g., the shell), a cooperative executor is used.

The global `SCHEDULER` is a `spin::Mutex` containing the scheduler state.

## 2. Preemptive Scheduler (`task/scheduler.rs`)

### 2.1 Scheduling Policy

-   **Priority-Based Round-Robin:**
    -   There are 8 priority levels (0=lowest, 7=highest).
    -   The scheduler maintains an array of 8 ready queues (`ready_queues`), one for each priority level.
    -   When scheduling, it picks a thread from the highest-priority non-empty queue.
    -   Threads at the same priority level are scheduled in a round-robin fashion.

### 2.2 Thread States

A `Thread` can be in one of the following states (`ThreadStatus`):

-   `Ready`: The thread is ready to run and is waiting in one of the `ready_queues`.
-   `Running`: The thread is currently executing on a CPU.
-   `Blocked`: The thread is waiting for an event (e.g., I/O, `sys_nanosleep`). Blocked threads are moved to a `sleep_queue`.
-   `Exited`: The thread has finished execution and is waiting to be reaped.

### 2.3 Context Switching

-   The `switch_context` function is implemented in raw x86_64 assembly.
-   It saves all general-purpose registers and the `rflags` register of the outgoing thread onto its stack.
-   It restores the registers of the incoming thread and uses a `ret` instruction to jump to its last known instruction pointer (`rip`).

## 3. SMP (Symmetric Multiprocessing)

-   The BSP (Bootstrap Processor) initializes the kernel and starts the APs (Application Processors) using the SIPI sequence.
-   Each AP performs its own initialization (GDT, IDT, LAPIC) and then enters the main scheduler loop (`task::scheduler::schedule()`).
-   All cores share the global `SCHEDULER` mutex. To prevent deadlocks, interrupt handlers use `try_lock` when interacting with the scheduler.

## 4. Async Executor (`task/executor.rs`)

-   A cooperative executor based on `crossbeam-queue` runs `async` tasks.
-   It is designed to run within a single preemptive `Thread`.
-   The kernel shell, GUI updates, and network polling are implemented as async tasks.
-   This allows for efficient, non-blocking I/O within the kernel without needing to block an entire kernel thread.
