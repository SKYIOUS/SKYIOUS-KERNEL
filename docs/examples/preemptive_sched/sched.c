#include "sched.h"
#include <stddef.h>

PCB *current_proc;
static PCB ready_dummy = { .next = &ready_dummy };
PCB *ready_list = &ready_dummy;                // circular list, dummy head

// ── intrusive linked-list queue ──

void enqueue(PCB *p) {
    p->next = ready_list;                      // p becomes new tail
    ready_list->next = p;                      // dummy → old head → … → p → dummy
    ready_list = p;                            // ponytail: sacrificial head pointer; replace with tail ptr if O(1) per-enqueue matters
}

PCB *dequeue_ready(void) {
    PCB *p = ready_list->next;                 // first real element
    if (p == ready_list)                       // empty (dummy points to self)
        return current_proc;                    // ponytail: idle same-process loop instead of idle thread
    ready_list->next = p->next;
    if (p == ready_list)                       // only element: reset tail
        ready_list = &ready_dummy;              // ponytail: see above
    p->next = NULL;
    return p;
}

// ── schedule — caller must hold IRQ disabled ──

void schedule(void) {
    PCB *old = current_proc;
    if (old->state == RUNNING) {
        old->state = READY;
        enqueue(old);
    }

    PCB *next = dequeue_ready();
    while (next == old) {                      // only process ready = spin
        next = dequeue_ready();
    }

    next->state = RUNNING;
    if (next != old)
        context_switch(old, next);
    // ponytail: per-thread TSS/FPU/VMX state saved here if ever needed
    //           (this kernel does not use FPU in kernel mode, so skip)
}
