use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
#[cfg(not(target_arch = "aarch64"))]
use crate::println;
#[cfg(not(target_arch = "aarch64"))]
#[cfg(not(target_arch = "aarch64"))]
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
#[cfg(not(target_arch = "aarch64"))]
use x86_64::structures::paging::PageTableFlags;
#[cfg(not(target_arch = "aarch64"))]
use pic8259::ChainedPics;

#[cfg(not(target_arch = "aarch64"))]
pub const PIC_1_OFFSET: u8 = 32;
#[cfg(not(target_arch = "aarch64"))]
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[cfg(not(target_arch = "aarch64"))]
// SAFETY: ChainedPics::new is safe when offsets are valid PIC interrupt offsets
pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Acquire)
}

#[cfg(not(target_arch = "aarch64"))]
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = 32,
    Keyboard = 33,
        _PageFault = 14,
    Mouse = 44,
    Network = 43,
    TlbFlush = 250,
    IpiFunc = 251,
}

#[cfg(not(target_arch = "aarch64"))]
impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

// ponytail: box-leaked IDT for 'static lifetime; raw ptr for interior mutability
#[cfg(not(target_arch = "aarch64"))]
struct IdtPtr(*mut InterruptDescriptorTable);
#[cfg(not(target_arch = "aarch64"))]
unsafe impl Send for IdtPtr {}
#[cfg(not(target_arch = "aarch64"))]
unsafe impl Sync for IdtPtr {}

#[cfg(not(target_arch = "aarch64"))]
static IDT: spin::Mutex<Option<IdtPtr>> = spin::Mutex::new(None);

#[cfg(not(target_arch = "aarch64"))]
pub fn init_idt() {
    use alloc::boxed::Box;
    let mut idt = Box::new(InterruptDescriptorTable::new());
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

    let raw = Box::into_raw(idt);
    // SAFETY: table is box-leaked (into_raw never freed), lives forever
    // load() is safe when IDT is properly configured
    unsafe { (*raw).load(); }
    *IDT.lock() = Some(IdtPtr(raw));

    unsafe {
        let mut pics = PICS.lock();
        pics.write_masks(0xFF, 0xFF);
        pics.initialize();
        pics.write_masks(0xFF, 0xFF);
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn init_ap() {
    if let Some(IdtPtr(ptr)) = *IDT.lock() {
        use x86_64::instructions::tables::{lidt, DescriptorTablePointer};
        use x86_64::VirtAddr;
        // SAFETY: table is box-leaked, never freed
        unsafe {
            let pointer = DescriptorTablePointer {
                base: VirtAddr::from_ptr(ptr as *const InterruptDescriptorTable),
                limit: (core::mem::size_of::<InterruptDescriptorTable>() - 1) as u16,
            };
            lidt(&pointer);
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
type MsiHandler = extern "x86-interrupt" fn(InterruptStackFrame);

#[cfg(not(target_arch = "aarch64"))]
pub fn set_handler(vector: u8, handler: MsiHandler) {
    if let Some(IdtPtr(ptr)) = *IDT.lock() {
        // SAFETY: single-core during registration; idt lives forever
        unsafe { (&mut *ptr)[vector as usize].set_handler_fn(handler); }
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn set_network_vector(vector: u8) {
    set_handler(vector, network_interrupt_handler);
    NET_VECTOR.store(vector, Ordering::Relaxed);
}

#[cfg(not(target_arch = "aarch64"))]
static NET_VECTOR: AtomicU8 = AtomicU8::new(InterruptIndex::Network as u8);

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
    // Clear CR0.TS (Task Switched) — this fires on lazy FPU context switch.
    // With +soft-float we don't use FPU, but some crates may emit FPU ops.
    unsafe {
        core::arch::asm!("clts", options(nostack, nomem));
    }
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64) -> !
{
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    let ticks = TICKS.fetch_add(1, Ordering::Release) + 1;

    crate::drivers::watchdog::pet();

    // Periodic diagnostic: print mouse state every 500 ticks (~5s)
    if ticks % 500 == 0 {
        let irq = crate::drivers::mouse::MOUSE_IRQ_COUNT.load(Ordering::Relaxed);
        let bytes = crate::drivers::mouse::MOUSE_IRQ_BYTES.load(Ordering::Relaxed);
        let cx = crate::drivers::mouse::CURSOR_X.load(Ordering::Relaxed);
        let cy = crate::drivers::mouse::CURSOR_Y.load(Ordering::Relaxed);
        crate::serial_write(&alloc::format!("[TICK={}] mouse irq={} bytes={} pos=({},{})\n", ticks, irq, bytes, cx, cy));
    }

    crate::apic::eoi();

    crate::task::scheduler::tick(ticks);
    crate::task::scheduler::try_schedule();
}

extern "x86-interrupt" fn tlb_flush_handler(
    _stack_frame: InterruptStackFrame)
{
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

    let cur = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref proc) = *cur {
        let page = x86_64::structures::paging::Page::containing_address(fault_addr);
        if let Some(true) = unsafe { proc.address_space.handle_cow(page) } {
            return;
        }
        if !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            if let Some(vma) = proc.find_vma(fault_addr.as_u64()) {
                use crate::memory::buddy::BuddyFrameAllocator;
                use x86_64::structures::paging::{Mapper, FrameAllocator};
                let mut fa = BuddyFrameAllocator;
                if let Some(frame) = fa.allocate_frame() {
                    if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                        let mut flags = vma.flags | PageTableFlags::PRESENT;

                        if fault_addr.as_u64() < 0x8000_0000_0000 {
                            flags |= PageTableFlags::USER_ACCESSIBLE;
                        }

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

    panic!(
        "PAGE FAULT at {:?}  error={:?}\n{:#?}",
        fault_addr, error_code, stack_frame
    );
}

extern "x86-interrupt" fn mouse_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    crate::drivers::mouse::MOUSE_IRQ_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    loop {
        let mut status_port = Port::<u8>::new(0x64);
        let status = unsafe { status_port.read() };
        if status & 1 == 0 {
            break;
        }
        let mut data_port = Port::<u8>::new(0x60);
        let byte = unsafe { data_port.read() };

        if status & 0x20 != 0 {
            crate::drivers::mouse::feed_byte(byte);
        } else {
            crate::keyboard::handle_scancode(byte);
            crate::tty::feed_scancode(byte);
        }
    }

    // EOI to slave PIC (IRQ12 is on slave) + master PIC
    // ExtINT mode: LAPIC is pass-through, PIC owns the interrupt lifecycle
    unsafe { Port::<u8>::new(0xA0).write(0x20); }
    unsafe { Port::<u8>::new(0x20).write(0x20); }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    // One-shot: print on first IRQ1 fire
    static KB_FIRED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if !KB_FIRED.swap(true, core::sync::atomic::Ordering::Relaxed) {
        crate::serial_write("[KBD] IRQ1 fired!\n");
    }

    loop {
        let mut status_port = Port::<u8>::new(0x64);
        let status = unsafe { status_port.read() };
        if status & 1 == 0 {
            break;
        }
        let mut data_port = Port::<u8>::new(0x60);
        let byte = unsafe { data_port.read() };

        if status & 0x20 != 0 {
            crate::drivers::mouse::feed_byte(byte);
        } else {
            crate::keyboard::handle_scancode(byte);
            crate::tty::feed_scancode(byte);
        }
    }

    // EOI to PIC (required for PIC→LINT0 ExtINT path)
    unsafe { Port::<u8>::new(0x20).write(0x20); }
}

extern "x86-interrupt" fn ipi_func_handler(
    _stack_frame: InterruptStackFrame)
{
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
    #[cfg(feature = "net")]
    {
        let icr = crate::drivers::net::NIC.lock().as_ref().map(|nic| {
            match nic {
                crate::drivers::net::NicDevice::E1000(dev) => {
                    dev.lock().inner.read_reg(crate::drivers::net::e1000::REG_ICR)
                }
                _ => 0,
            }
        }).unwrap_or(0);

        if icr == 0 {
            crate::apic::eoi();
            return;
        }

        crate::net::poll();
    }
    crate::apic::eoi();
}
