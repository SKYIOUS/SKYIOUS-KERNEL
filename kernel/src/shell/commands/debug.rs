use crate::println;
use crate::vga_buffer::{self, Color};

pub fn heap_test() {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    
    vga_buffer::set_color(Color::LightGreen, Color::Black);
    println!("Running Heap Test...");
    let x = Box::new(5);
    println!("Boxed value: {} at {:p}", x, x);
    
    let mut v = Vec::new();
    for i in 0..100 {
        v.push(i);
    }
    println!("Vector sum: {} at {:p}", v.iter().sum::<i32>(), v.as_ptr());
    println!("Heap test passed!");
    vga_buffer::set_color(Color::White, Color::Black);
}

pub fn lspci() {
    crate::pci::enumerate_pci();
}

pub fn panic() {
    panic!("User requested panic!");
}

pub fn test_pf() {
    use crate::task::process::{Process, Vma, CURRENT_PROCESS};
    use crate::memory::paging::AddressSpace;
    use x86_64::structures::paging::PageTableFlags;
    use alloc::sync::Arc;

    println!("[TEST] Demand Paging...");

    let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
    let address_space = AddressSpace::new(&mut frame_allocator).expect("Failed to create AddressSpace");
    let process = Process::new(2, None, address_space);

    let test_addr = 0x1234_5678_0000u64;
    process.add_vma(Vma {
        start: test_addr,
        end: test_addr + 4096,
        flags: PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
        _name: "Test Demand Paging",
    });

    let process_arc = Arc::new(process);
    {
        let mut cur = CURRENT_PROCESS.lock();
        *cur = Some(process_arc.clone());
    }

    unsafe {
        process_arc.address_space.activate();
    }

    println!("[TEST] Attempting write to 0x{:x}...", test_addr);
    
    let ptr = test_addr as *mut u64;
    unsafe {
        *ptr = 0xCAFEBABE_DEADBEEF;
    }

    let val = unsafe { *ptr };
    println!("[TEST] Value at 0x{:x} is 0x{:x}", test_addr, val);
    
    if val == 0xCAFEBABE_DEADBEEF {
        println!("[TEST] Demand Paging: SUCCESS ✅");
    } else {
        println!("[TEST] Demand Paging: FAILED ❌");
    }
}

pub fn test_cow() {
    use crate::task::process::{Process, Vma, CURRENT_PROCESS};
    use crate::memory::paging::AddressSpace;
    use x86_64::structures::paging::{PageTableFlags, Page, Size4KiB, Mapper, FrameAllocator};
    use alloc::sync::Arc;
    use spin::Mutex;
    use crate::memory::buddy::BuddyFrameAllocator;

    println!("[TEST] Copy-on-Write...");

    let mut frame_allocator = BuddyFrameAllocator;
    
    let parent_as = AddressSpace::new(&mut frame_allocator).expect("Failed to create parent AS");
    let parent = Process::new(10, None, parent_as);
    
    let test_addr = 0x2222_3333_0000u64;
    parent.add_vma(Vma {
        start: test_addr,
        end: test_addr + 4096,
        flags: PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
        _name: "COW Test Region",
    });

    {
        let mut mapper = unsafe { parent.address_space.mapper().expect("Failed to get mapper") };
        let page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(test_addr));
        let frame = frame_allocator.allocate_frame().unwrap();
        unsafe {
            mapper.map_to(page, frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE, &mut frame_allocator)
                .unwrap().flush();
            let ptr = test_addr as *mut u64;
            *ptr = 0x1111_1111_1111_1111;
        }
    }

    println!("[TEST] Cloning Address Space (COW)...");
    let child_as = parent.address_space.clone_cow(&mut frame_allocator).expect("Failed to clone AS");
    let mut child = Process::new(11, None, child_as);
    {
        let parent_vmas = parent.vmas.lock();
        child.vmas = Mutex::new(parent_vmas.clone());
    }
    let parent_arc = Arc::new(parent);
    let child_arc = Arc::new(child);

    println!("[TEST] Verifying child can read parent's value...");
    unsafe { child_arc.address_space.activate(); }
    {
        let mut cur = CURRENT_PROCESS.lock();
        *cur = Some(child_arc.clone());
    }

    let val = unsafe { *(test_addr as *const u64) };
    println!("[TEST] Child read value: 0x{:x}", val);
    assert_eq!(val, 0x1111_1111_1111_1111);

    println!("[TEST] Attempting write in child to trigger COW...");
    unsafe {
        crate::memory::copy_to_user(test_addr as *mut u8, b"Demand Paging Test SUCCESS!", 27);
    }

    let val_child = unsafe { *(test_addr as *const u64) };
    println!("[TEST] Child value after write: 0x{:x}", val_child);
    assert_eq!(val_child, 0x2222_2222_2222_2222);

    println!("[TEST] Verifying parent memory is unchanged...");
    unsafe { parent_arc.address_space.activate(); }
    {
        let mut cur = CURRENT_PROCESS.lock();
        *cur = Some(parent_arc.clone());
    }
    let val_parent = unsafe { *(test_addr as *const u64) };
    println!("[TEST] Parent value: 0x{:x}", val_parent);
    
    if val_parent == 0x1111_1111_1111_1111 && val_child == 0x2222_2222_2222_2222 {
        println!("[TEST] Copy-on-Write: SUCCESS ✅");
    } else {
        println!("[TEST] Copy-on-Write: FAILED ❌");
    }
}
