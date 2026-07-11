#ifndef SCHED_H
#define SCHED_H

#include <stdint.h>

#define READY   0
#define RUNNING 1
#define BLOCKED 2
#define DEAD    3

typedef struct pcb {
    uint64_t rsp, rip, rflags;
    uint64_t rbx, rbp, r12, r13, r14, r15;
    uint64_t cr3;
    int state;
    struct pcb *next;              // intrusive linked list
} PCB;

extern PCB *current_proc;
extern PCB *ready_list;            // dummy head of circular linked list

void enqueue(PCB *p);
PCB *dequeue_ready(void);
void schedule(void);               // called at end of timer interrupt
void context_switch(PCB *old, PCB *new_);

// ponytail: fixed-priority levels omitted; add if real-time tasks appear

#endif
