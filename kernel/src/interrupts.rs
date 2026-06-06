use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::structures::paging::PageTableFlags;
use crate::println;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

static TICKS: spin::Mutex<u64> = spin::Mutex::new(0);

pub fn get_ticks() -> u64 {
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        *TICKS.lock()
    })
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = 32,
    Keyboard = 33,
        _PageFault = 14,
    Mouse = 44,
    Network = 43, // IRQ 11 (mapped to PIC2+3)
    TlbFlush = 250,
    IpiFunc = 251,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);

        idt[InterruptIndex::Timer.as_usize()]
            .set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()]
            .set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Mouse.as_usize()]
            .set_handler_fn(mouse_interrupt_handler);
        idt[InterruptIndex::Network.as_usize()]
            .set_handler_fn(network_interrupt_handler);
        idt[InterruptIndex::TlbFlush.as_usize()]
            .set_handler_fn(tlb_flush_handler);
        idt[InterruptIndex::IpiFunc.as_usize()]
            .set_handler_fn(ipi_func_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
    // Disable legacy PIC
    unsafe {
        let mut pics = PICS.lock();
        // Mask all interrupts on both PICs
        pics.write_masks(0xFF, 0xFF);
        // Then initialize and mask again just to be sure it's quiet
        pics.initialize();
        pics.write_masks(0xFF, 0xFF);
    }
}

pub fn init_ap() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: InterruptStackFrame)
{
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame, error_code: u64)
{
    panic!("EXCEPTION: GENERAL PROTECTION FAULT (error_code: {})\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame, error_code: u64)
{
    panic!("EXCEPTION: STACK SEGMENT FAULT (error_code: {})\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(
    stack_frame: InterruptStackFrame)
{
    panic!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn device_not_available_handler(
    _stack_frame: InterruptStackFrame)
{
    // Necessary for FPU/SSE lazy loading if implemented, otherwise just panic for now
    panic!("EXCEPTION: DEVICE NOT AVAILABLE (NM)");
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64) -> !
{
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    let ticks = {
        let mut ticks = TICKS.lock();
        *ticks += 1;
        *ticks
    };

    // Pet watchdog every tick regardless of scheduler lock state
    crate::drivers::watchdog::pet();

    // Send EOI before any context switch so LAPIC stays alive
    crate::apic::eoi();

    // Update scheduler for sleeping threads and trigger preemption
    crate::task::scheduler::tick(ticks);
    crate::task::scheduler::try_schedule();
}

extern "x86-interrupt" fn tlb_flush_handler(
    _stack_frame: InterruptStackFrame)
{
    // A simple way to flush TLB on x86_64 is to reload CR3
    unsafe {
        use x86_64::registers::control::Cr3;
        let (frame, flags) = Cr3::read();
        Cr3::write(frame, flags);
    }
    crate::apic::eoi();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    let fault_addr = Cr2::read();
    
    // 1. Try COW resolution
    let cur = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref proc) = *cur {
        let page = x86_64::structures::paging::Page::containing_address(fault_addr);
        if let Some(true) = unsafe { proc.address_space.handle_cow(page) } {
            return; // COW resolved — iretq back
        }
        // 2. Try demand paging (valid VMA, page not present)
        if !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            if let Some(vma) = proc.find_vma(fault_addr.as_u64()) {
                // Allocate frame and map it
                use crate::memory::buddy::BuddyFrameAllocator;
                use x86_64::structures::paging::{Mapper, FrameAllocator};
                let mut fa = BuddyFrameAllocator;
                if let Some(frame) = fa.allocate_frame() {
                    if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                        let mut flags = vma.flags | PageTableFlags::PRESENT;
                        
                        // Ensure USER_ACCESSIBLE is set for user VMAs
                        if fault_addr.as_u64() < 0x8000_0000_0000 {
                            flags |= PageTableFlags::USER_ACCESSIBLE;
                        }

                        let _ = unsafe { mapper.map_to(page, frame, flags, &mut fa).map(|f| f.flush()) };
                        crate::memory::frame_info::increment(frame.start_address());
                        
                        // Zero the new page
                        let virt = x86_64::VirtAddr::new(
                            *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap()
                            + frame.start_address().as_u64()
                        );
                        unsafe { core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, 4096); }
                        return;
                    }
                }
            }
            // 2a. Try demand paging for brk region
            let fault_u64 = fault_addr.as_u64();
            if fault_u64 >= 0x6000_0000_0000 && fault_u64 < *proc.brk.lock() {
                use crate::memory::buddy::BuddyFrameAllocator;
                use x86_64::structures::paging::{Mapper, FrameAllocator};
                let mut fa = BuddyFrameAllocator;
                if let Some(frame) = fa.allocate_frame() {
                    if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
                        let _ = unsafe { mapper.map_to(page, frame, flags, &mut fa).map(|f| f.flush()) };
                        crate::memory::frame_info::increment(frame.start_address());
                        let virt = x86_64::VirtAddr::new(
                            *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap()
                            + frame.start_address().as_u64()
                        );
                        unsafe { core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, 4096); }
                        return;
                    }
                }
            }
        }
    }
    drop(cur);

    // 3. Not resolvable — kernel panic
    panic!(
        "PAGE FAULT at {:?}  error={:?}\n{:#?}",
        fault_addr, error_code, stack_frame
    );
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    let mut status_port = Port::<u8>::new(0x64);
    let status = unsafe { status_port.read() };
    let mut data_port = Port::<u8>::new(0x60);
    let byte = unsafe { data_port.read() };

    if status & 0x20 != 0 {
        // Byte is from the mouse — route to mouse state machine
        crate::drivers::mouse::feed_byte(byte);
    } else {
        // Byte is from the keyboard
        crate::keyboard::handle_scancode(byte);
        crate::tty::feed_scancode(byte);
    }

    crate::apic::eoi();
}

extern "x86-interrupt" fn mouse_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    let mut status_port = Port::<u8>::new(0x64);
    let status = unsafe { status_port.read() };
    let mut data_port = Port::<u8>::new(0x60);
    let byte = unsafe { data_port.read() };

    if status & 0x20 != 0 {
        // Byte is from the mouse
        crate::drivers::mouse::feed_byte(byte);
    } else {
        // Byte is from the keyboard — route to keyboard handler
        crate::keyboard::handle_scancode(byte);
        crate::tty::feed_scancode(byte);
    }

    crate::apic::eoi();
}

extern "x86-interrupt" fn ipi_func_handler(
    _stack_frame: InterruptStackFrame)
{
    // Execute a function queued via smp_call_function.
    // The function pointer and argument are in per-CPU data.
    let cpu = crate::syscalls::get_per_cpu();
    if cpu.ipi_pending != 0 {
        let func: extern "C" fn(u64) = unsafe { core::mem::transmute(cpu.ipi_pending) };
        let arg = cpu.ipi_arg;
        cpu.ipi_pending = 0;
        cpu.ipi_arg = 0;
        func(arg);
    }
    crate::apic::eoi();
}

extern "x86-interrupt" fn network_interrupt_handler(
    _stack_frame: InterruptStackFrame) 
{
    // Drive the network stack
    #[cfg(feature = "net")]
    crate::net::poll();
    crate::apic::eoi();
}
