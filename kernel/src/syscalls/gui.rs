#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
//! gui syscalls — split from mod.rs (7246 lines).
use super::errno;
use super::numbers;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::vec;
use crate::gui::{COMPOSITOR, window::Window};
use crate::memory::buddy::{BUDDY_ALLOCATOR, MAX_ORDER};
use crate::memory::physical_memory_offset;

pub fn sys_beep(freq_hz: u32, duration_ms: u32) -> u64 {
    crate::drivers::audio::pcspeaker::beep(freq_hz, duration_ms);
    0
}

pub fn sys_gui_create_window(title_ptr: *const u8, width: usize, height: usize) -> u64 {
    use crate::gui::{COMPOSITOR, window::Window};
    let mut comp = COMPOSITOR.lock();
    
    let title_str = if title_ptr.is_null() {
        alloc::string::String::from("User App").into_boxed_str()
    } else {
        match unsafe { crate::syscalls::user_access::read_user_string(title_ptr, 256) } {
            Ok(s) => s.into_boxed_str(),
            Err(_) => alloc::string::String::from("User App").into_boxed_str(),
        }
    };

    let mut win = Window::new(0, 0, width + 2, height + 22, &title_str);
    
    // PHASE G3: Allocate shared physical memory for high-performance rendering
    let content_len = width * height;
    let size_bytes = content_len * 4;
    
    use crate::memory::buddy::BUDDY_ALLOCATOR;
    let mut order = 0;
    while (4096 << order) < size_bytes && order < crate::memory::buddy::MAX_ORDER {
        order += 1;
    }

    // Try contiguous first, then fall back to content (which needs copy in flush)
    let phys_addr = BUDDY_ALLOCATOR.lock().allocate_contiguous(order);
    if let Some(pa) = phys_addr {
        win.phys_addr = Some(pa.as_u64());
        let offset = crate::memory::physical_memory_offset();
        let k_ptr = (offset + pa.as_u64()) as *mut u8;
        unsafe { core::ptr::write_bytes(k_ptr, 0, (4096 << order) as usize); }
    } else {
        win.content = Some(alloc::vec![0; content_len].into_boxed_slice());
    }
    
    comp.add_window(win);
    (comp.windows.len() - 1) as u64 // Handle
}

pub fn sys_gui_get_buffer(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }

    let win = &comp.windows[handle as usize];
    let content_w = win.width.saturating_sub(2);
    let content_h = win.height.saturating_sub(22);

    // Pack width and height into return value (low 32 = width, high 32 = height)
    ((content_w as u64) & 0xFFFF_FFFF) | ((content_h as u64) << 32)
}

pub fn sys_gui_map_buffer(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }
    
    let win = &comp.windows[handle as usize];
    let phys_addr = match win.phys_addr {
        Some(p) => p,
        None => return 0,
    };

    let content_w = win.width.saturating_sub(2);
    let content_h = win.height.saturating_sub(22);
    let size_bytes = content_w * content_h * 4;
    let pages_needed = size_bytes.div_ceil(4096);

    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return 0 };

    // Find a virtual address to map to
    static NEXT_GUI_MAP_ADDR: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0x5000_0000_0000);
    let v_addr = NEXT_GUI_MAP_ADDR.fetch_add(pages_needed as u64 * 4096, core::sync::atomic::Ordering::SeqCst);

    use crate::memory::buddy::BuddyFrameAllocator;
    let mut frame_allocator = BuddyFrameAllocator;
    let mut mapper = if let Some(m) = unsafe { process.address_space.mapper() } { m } else { return 0; };

    for i in 0..pages_needed {
        let page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(v_addr + i as u64 * 4096));
        let frame = x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(phys_addr + i as u64 * 4096));
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        
        unsafe {
            if let Ok(t) = mapper.map_to(page, frame, flags, &mut frame_allocator) {
                t.flush();
            }
        }
    }

    process.add_vma(crate::task::process::Vma {
        start: v_addr,
        end: v_addr + pages_needed as u64 * 4096,
        flags: PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
        _name: "gui_buffer",
        file_handle: None,
        file_offset: 0,
        is_shared: false,
        shm_id: None,
    });

    v_addr
}

pub fn sys_gui_flush(handle: u64, buf_ptr: *const u32) -> u64 {
    use crate::gui::COMPOSITOR;
    use core::sync::atomic::Ordering;
    let (mx, my) = (
        crate::drivers::mouse::CURSOR_X.load(Ordering::Relaxed) as usize,
        crate::drivers::mouse::CURSOR_Y.load(Ordering::Relaxed) as usize,
    );
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EBADF as u64; }

    let win = &mut comp.windows[handle as usize];
    if win.phys_addr.is_some() {
        // Zero copy: buffer is already updated by user
    } else if let Some(ref mut content) = win.content {
        let len = content.len();
        if !buf_ptr.is_null() {
            unsafe {
                let _ = crate::syscalls::user_access::copy_from_user(
                    core::slice::from_raw_parts_mut(content.as_mut_ptr() as *mut u8, len * 4),
                    buf_ptr as *const u8,
                );
            }
        }
    } else {
        return errno::Errno::ENOSYS as u64;
    }
    comp.render(mx, my);
    0
}

pub fn sys_gui_get_key(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }
    let win = &mut comp.windows[handle as usize];
    win.key_events.pop_front().map(|k| k as u64).unwrap_or(0)
}

pub fn sys_gui_get_mouse(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    use core::sync::atomic::Ordering;
    let comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }
    let win = &comp.windows[handle as usize];
    let mx = crate::drivers::mouse::CURSOR_X.load(Ordering::Relaxed) as i64;
    let my = crate::drivers::mouse::CURSOR_Y.load(Ordering::Relaxed) as i64;
    let buttons = crate::drivers::mouse::CURSOR_BUTTONS.load(Ordering::Relaxed) as u64;
    let scroll = crate::drivers::mouse::CURSOR_SCROLL.load(Ordering::Relaxed) as i64;
    // Return mouse position relative to window content area
    let rel_x = (mx - win.x as i64 - 1).max(0) as u64;
    let rel_y = (my - win.y as i64 - 21).max(0) as u64;
    let scroll = scroll as u64;
    // Pack: low16=x, bits16-31=y, bits32-39=buttons, bits40-47=scroll
    (rel_x & 0xFFFF) | ((rel_y & 0xFFFF) << 16) | ((buttons & 0xFF) << 32) | ((scroll & 0xFF) << 40)
}

pub fn sys_gui_set_title(handle: u64, title_ptr: *const u8) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    let win = &mut comp.windows[handle as usize];
    if title_ptr.is_null() { return errno::Errno::EINVAL as u64; }
    let mut len = 0;
    unsafe {
        while *title_ptr.add(len) != 0 && len < 64 { len += 1; }
    }
    let title_slice = unsafe { core::slice::from_raw_parts(title_ptr, len) };
    if let Ok(s) = core::str::from_utf8(title_slice) {
        win.title = alloc::string::String::from(s).into_boxed_str();
    }
    0
}

pub fn sys_gui_destroy_window(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    comp.windows.remove(handle as usize);
    0
}

pub fn sys_gui_resize_window(handle: u64, width: u64, height: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    let win = &mut comp.windows[handle as usize];
    win.width = width as usize;
    win.height = height as usize;
    0
}

pub fn sys_gui_move_window(handle: u64, x: u64, y: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    let win = &mut comp.windows[handle as usize];
    win.x = x as usize;
    win.y = y as usize;
    0
}

pub fn sys_clipboard(mode: u64, buf: *mut u8, len: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    match mode {
        0 => {
            // Read clipboard
            let copy_len = (len as usize).min(comp.clipboard.len());
            if copy_len == 0 { return 0; }
            if buf.is_null() { return comp.clipboard.len() as u64; }
            unsafe {
                core::ptr::copy_nonoverlapping(comp.clipboard.as_ptr(), buf, copy_len);
            }
            copy_len as u64
        }
        1 => {
            // Write clipboard
            if buf.is_null() || len == 0 { comp.clipboard.clear(); return 0; }
            let mut new_data = alloc::vec![0u8; len as usize];
            unsafe {
                core::ptr::copy_nonoverlapping(buf, new_data.as_mut_ptr(), len as usize);
            }
            comp.clipboard = new_data;
            len
        }
        2 => {
            // Get clipboard length
            comp.clipboard.len() as u64
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

pub fn sys_notify(text_ptr: *const u8, duration_ms: u64, kind: u64) -> u64 {
    use crate::gui::{COMPOSITOR, NotifKind};
    if text_ptr.is_null() { return errno::Errno::EINVAL as u64; }
    let mut len = 0;
    unsafe {
        while *text_ptr.add(len) != 0 && len < 256 { len += 1; }
    }
    let text_slice = unsafe { core::slice::from_raw_parts(text_ptr, len) };
    let text = match core::str::from_utf8(text_slice) {
        Ok(s) => alloc::string::String::from(s),
        Err(_) => return errno::Errno::EINVAL as u64,
    };
    let notif_kind = match kind {
        1 => NotifKind::Warning,
        2 => NotifKind::Error,
        _ => NotifKind::Info,
    };
    let ticks = (duration_ms / 10).max(10);
    let mut comp = COMPOSITOR.lock();
    comp.notifications.push(crate::gui::Notification {
        text,
        kind: notif_kind,
        ticks_remaining: ticks,
        x: 0,
        y: 0,
    });
    0
}

pub fn sys_drmctl(_fd: u64, request: u64, arg: *mut u8) -> u64 {
    const DRM_IOCTL_GET_DISPLAY_INFO: u64 = 0x0100;
    const DRM_IOCTL_CREATE_DUMB: u64 = 0x0101;
    const DRM_IOCTL_DESTROY_DUMB: u64 = 0x0103;
    const DRM_IOCTL_FLIP: u64 = 0x0104;
    const DRM_IOCTL_SET_MODE: u64 = 0x0105;
    const DRM_IOCTL_MAP_DUMB: u64 = 0x0106;
    const DRM_IOCTL_PAGE_FLIP: u64 = 0x0107;
    const DRM_IOCTL_GEM_CREATE: u64 = 0x0108;
    const DRM_IOCTL_GEM_MMAP: u64 = 0x0109;

    match request {
        DRM_IOCTL_GET_DISPLAY_INFO => {
            #[repr(C)]
            struct DisplayInfo { width: u32, height: u32 }
            let info = DisplayInfo {
                width: crate::drivers::gpu::width(),
                height: crate::drivers::gpu::height(),
            };
            let bytes = unsafe {
                core::slice::from_raw_parts(&info as *const DisplayInfo as *const u8, core::mem::size_of::<DisplayInfo>())
            };
            if unsafe { user_access::copy_to_user(arg, bytes).is_err() } {
                return errno::Errno::EFAULT as u64;
            }
            0
        }
        DRM_IOCTL_CREATE_DUMB => {
            use alloc::boxed::Box;
            use alloc::vec;
            let w = crate::drivers::gpu::width();
            let h = crate::drivers::gpu::height();
            let fb: &'static mut [u32] = Box::leak(vec![0u32; (w * h) as usize].into_boxed_slice());
            let paddr = match crate::memory::virt_to_phys(VirtAddr::from_ptr(fb.as_ptr())) {
                Some(pa) => pa.as_u64(),
                None => {
                    crate::serial_write("[GUI] Failed to translate framebuffer address\n");
                    return errno::Errno::EFAULT as u64;
                }
            };
            #[repr(C)]
            struct DumbInfo { id: u64, size: u64, addr: u64 }
            let di = DumbInfo { id: 1, size: (w * h * 4) as u64, addr: paddr };
            let bytes = unsafe {
                core::slice::from_raw_parts(&di as *const DumbInfo as *const u8, core::mem::size_of::<DumbInfo>())
            };
            if unsafe { user_access::copy_to_user(arg, bytes).is_err() } {
                return errno::Errno::EFAULT as u64;
            }
            0
        }
        DRM_IOCTL_DESTROY_DUMB => {
            0 // Memory will be freed on process exit; for now, no-op
        }
        DRM_IOCTL_FLIP => {
            crate::drivers::gpu::virtio_gpu::flip();
            0
        }
        DRM_IOCTL_SET_MODE => {
            // arg1=width, arg2=height (passed as direct args from userspace)
            let new_w = _fd as usize;
            let new_h = request as usize;
            if !(640..=3840).contains(&new_w) || !(480..=2160).contains(&new_h) {
                return errno::Errno::EINVAL as u64;
            }
            crate::drivers::gpu::set_mode(new_w as u32, new_h as u32);
            crate::drivers::graphics::WIDTH.store(new_w, core::sync::atomic::Ordering::SeqCst);
            crate::drivers::graphics::HEIGHT.store(new_h, core::sync::atomic::Ordering::SeqCst);
            crate::drivers::graphics::STRIDE.store(new_w, core::sync::atomic::Ordering::SeqCst);
            crate::gui::COMPOSITOR.lock().set_resolution(new_w, new_h);
            crate::println!("DRM: set_mode {}x{}", new_w, new_h);
            0
        }
        DRM_IOCTL_MAP_DUMB => {
            // Return the virtual address of the framebuffer
            let fb_ptr = crate::drivers::graphics::FRAMEBUFFER.load(core::sync::atomic::Ordering::Relaxed);
            fb_ptr as u64
        }
        DRM_IOCTL_PAGE_FLIP => {
            // Flip to a specific buffer (id in arg1)
            crate::drivers::gpu::virtio_gpu::flip();
            0
        }
        DRM_IOCTL_GEM_CREATE => {
            // Allocate a GEM object of `_fd` bytes size
            use alloc::boxed::Box;
            use alloc::vec;
            let size = _fd as usize;
            if size == 0 || size > 64 * 1024 * 1024 {
                return errno::Errno::EINVAL as u64;
            }
            let buf: &'static mut [u8] = Box::leak(vec![0u8; size].into_boxed_slice());
            buf.as_ptr() as u64
        }
        DRM_IOCTL_GEM_MMAP => {
            // id is the address returned by GEM_CREATE (the kernel buffer address)
            // Return it as the mmap address
            _fd
        }
        // 0x010A = SET_ACCENT_COLOR: arg = packed ARGB u32
        0x010A => {
            let color = arg as u32 | 0xFF000000;
            unsafe { crate::gui::ACCENT_COLOR = color; }
            crate::println!("DRM: accent color -> 0x{:08X}", color);
            0
        }
        // 0x010B = SET_WALLPAPER: arg = path string pointer
        0x010B => {
            let path = match unsafe { user_access::read_user_string(arg, 256) } {
                Ok(s) => s,
                Err(_) => return errno::Errno::EFAULT as u64,
            };
            let mut comp = crate::gui::COMPOSITOR.lock();
            comp.set_wallpaper(path);
            crate::println!("DRM: wallpaper path set");
            0
        }
        _ => errno::Errno::ENOSYS as u64,
    }
}

pub fn sys_openpty() -> u64 {
    let (idx, pair) = match crate::pty::alloc_pty() {
        Some(p) => p,
        None => return errno::Errno::ENFILE as u64,
    };
    let proc_lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        let mut ft = proc.fd_table.lock();
        let m = ft.iter().position(|f| f.is_none()).unwrap_or(ft.len());
        if m >= 256 { return errno::Errno::ENFILE as u64; }
        if m == ft.len() { ft.push(None); }
        ft[m] = Some(FileDescriptor::PtyMaster { _idx: idx, pair: pair.clone() });
        let s = ft.iter().position(|f| f.is_none()).unwrap_or(ft.len());
        if s >= 256 { ft[m] = None; return errno::Errno::ENFILE as u64; }
        if s == ft.len() { ft.push(None); }
        ft[s] = Some(FileDescriptor::PtySlave { _idx: idx, pair });
        (m as u64) | ((s as u64) << 16)
    } else {
        errno::Errno::ENOTTY as u64
    }
}
