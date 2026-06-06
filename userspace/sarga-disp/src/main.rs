#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::ffi::CString;
use alloc::boxed::Box;
use libsarga::{Color, Backbuffer, Framebuffer, Event, App};
use libsarga::app::{SkyTerm, SkyCalc};
use libsarga::event::MouseState;

const SCREEN_W: usize = 800;
const SCREEN_H: usize = 600;
const TASKBAR_H: usize = 32;
const TITLEBAR_H: usize = 20;

struct Win {
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    app: Box<dyn App>,
    dragging: bool,
    drag_off_x: i32,
    drag_off_y: i32,
}

fn zorder_test(wins: &[Win], mx: i32, my: i32) -> Option<usize> {
    for i in (0..wins.len()).rev() {
        let w = &wins[i];
        if mx >= w.x && mx < w.x + w.w as i32 && my >= w.y && my < w.y + w.h as i32 {
            return Some(i);
        }
    }
    None
}

fn hit_titlebar(w: &Win, mx: i32, my: i32) -> bool {
    mx >= w.x && mx < w.x + w.w as i32 && my >= w.y && my < w.y + TITLEBAR_H as i32
}

fn draw_titlebar(bb: &mut Backbuffer, win: &Win, active: bool) {
    let tb_color = if active { Color::NAVY } else { Color::DARK_GRAY };
    let x = win.x as usize;
    let y = win.y as usize;
    bb.fill_rect(x, y, win.w, TITLEBAR_H, tb_color);
    bb.draw_rect(x, y, win.w, TITLEBAR_H, Color::WHITE);
    let text_x = x + 4;
    let text_y = y + (TITLEBAR_H - 8) / 2;
    let title = win.app.title();
    bb.draw_text(text_x, text_y, title, Color::WHITE, tb_color);
}

fn draw_window(bb: &mut Backbuffer, win: &Win, active: bool) {
    draw_titlebar(bb, win, active);
    let x = win.x as usize;
    let y = win.y as usize;
    let cx = x + 2;
    let cy = y + TITLEBAR_H + 2;
    let cw = win.w - 4;
    let ch = win.h - TITLEBAR_H - 4;
    win.app.render(bb, cx, cy, cw, ch);
}

fn draw_taskbar(bb: &mut Backbuffer, win_count: usize, active_idx: usize) {
    bb.fill_rect(0, SCREEN_H - TASKBAR_H, SCREEN_W, TASKBAR_H, Color::DARK_GRAY);
    bb.draw_rect(0, SCREEN_H - TASKBAR_H, SCREEN_W, TASKBAR_H, Color::LIGHT_GRAY);
    for i in 0..win_count {
        let btn_x = 8 + i * 120;
        let btn_y = SCREEN_H - TASKBAR_H + 4;
        let bg = if i == active_idx { Color::NAVY } else { Color::GRAY };
        bb.fill_rect(btn_x, btn_y, 110, TASKBAR_H - 8, bg);
        bb.draw_rect(btn_x, btn_y, 110, TASKBAR_H - 8, Color::LIGHT_GRAY);
        let label = alloc::format!("App {}", i + 1);
        bb.draw_text(btn_x + 4, btn_y + ((TASKBAR_H - 8) - 8) / 2, &label, Color::WHITE, bg);
    }
}

fn draw_desktop(bb: &mut Backbuffer) {
    bb.fill_rect(0, 0, SCREEN_W, SCREEN_H, Color::rgb(0, 100, 100));
    bb.draw_text(10, 10, "SkyOS Desktop Environment", Color::WHITE, Color::rgb(0, 100, 100));
    bb.draw_text(10, 25, "ESC: exit  TAB: switch  Mouse: click/drag  Start: desktop click", Color::LIGHT_GRAY, Color::rgb(0, 100, 100));
}

fn draw_start_menu(bb: &mut Backbuffer, show: bool, mx: i32, my: i32) {
    if !show { return; }
    let menu_x = 0;
    let menu_y = (SCREEN_H - TASKBAR_H) as i32 - 200;
    let menu_w = 180;
    let menu_h = 200;
    bb.fill_rect(menu_x as usize, menu_y as usize, menu_w, menu_h, Color::DARK_GRAY);
    bb.draw_rect(menu_x as usize, menu_y as usize, menu_w, menu_h, Color::WHITE);
    let items = ["Terminal", "Calculator", "Files", "Settings", "About"];
    for (i, item) in items.iter().enumerate() {
        let iy = menu_y as usize + 4 + i * 36;
        let highlight = mx >= menu_x && mx < menu_x + menu_w as i32 && my >= iy as i32 && my < (iy + 36) as i32;
        if highlight {
            bb.fill_rect(menu_x as usize, iy, menu_w, 36, Color::NAVY);
        }
        bb.draw_text(menu_x as usize + 8, iy + 14, item, Color::WHITE, if highlight { Color::NAVY } else { Color::DARK_GRAY });
    }
}

fn draw_mouse_cursor(bb: &mut Backbuffer, mx: i32, my: i32) {
    let x = mx as usize;
    let y = my as usize;
    let cursor: [[u8; 12]; 12] = [
        [1,0,0,0,0,0,0,0,0,0,0,0],
        [1,1,0,0,0,0,0,0,0,0,0,0],
        [1,1,1,0,0,0,0,0,0,0,0,0],
        [1,1,1,1,0,0,0,0,0,0,0,0],
        [1,1,1,1,1,0,0,0,0,0,0,0],
        [1,1,1,1,1,1,0,0,0,0,0,0],
        [1,1,1,1,1,1,1,0,0,0,0,0],
        [1,1,1,1,1,1,1,1,0,0,0,0],
        [1,1,1,1,1,1,1,1,1,0,0,0],
        [1,1,1,1,1,1,0,0,0,0,0,0],
        [1,1,0,0,1,1,0,0,0,0,0,0],
        [1,0,0,0,0,1,1,0,0,0,0,0],
    ];
    for row in 0..12 {
        for col in 0..12 {
            if cursor[row][col] != 0 {
                let px = x.wrapping_add(col);
                let py = y.wrapping_add(row);
                if px < SCREEN_W && py < SCREEN_H {
                    bb.set_px(px, py, Color::WHITE);
                }
            }
        }
    }
}

fn read_input_events(fd: u64, buffer: &mut [u8]) -> usize {
    let nread = skyos_libc::syscall::read(fd, buffer);
    if (nread as i64) > 0 { nread as usize } else { 0 }
}

fn parse_input_events(buf: &[u8], len: usize, mouse: &mut MouseState) -> alloc::vec::Vec<Event> {
    let mut events = alloc::vec::Vec::new();
    let mut off = 0;
    while off + 8 <= len {
        let kind = u16::from_le_bytes([buf[off], buf[off + 1]]);
        let code = u16::from_le_bytes([buf[off + 2], buf[off + 3]]);
        let value = i32::from_le_bytes([buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7]]);
        off += 8;
        match kind {
            1 => { // EV_KEY
                if value == 1 {
                    events.push(Event::KeyDown(code));
                } else if value == 0 {
                    events.push(Event::KeyUp(code));
                }
            }
            2 => { // EV_REL
                match code {
                    0 => { // REL_X
                        mouse.x = core::cmp::max(0, core::cmp::min((SCREEN_W - 1) as i32, mouse.x + value));
                        events.push(Event::MouseMove(mouse.x, mouse.y));
                    }
                    1 => { // REL_Y
                        mouse.y = core::cmp::max(0, core::cmp::min((SCREEN_H - 1) as i32, mouse.y + value));
                        events.push(Event::MouseMove(mouse.x, mouse.y));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    events
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let fb = match Framebuffer::open() {
        Some(f) => f,
        None => return 1,
    };
    let mut bb = Backbuffer::new(SCREEN_W, SCREEN_H);
    let mut mouse = MouseState::new();

    let mut wins: Vec<Win> = Vec::new();
    wins.push(Win {
        x: 50, y: 50, w: 600, h: 420,
        app: Box::new(SkyTerm::new()),
        dragging: false, drag_off_x: 0, drag_off_y: 0,
    });
    wins.push(Win {
        x: 350, y: 160, w: 320, h: 380,
        app: Box::new(SkyCalc::new()),
        dragging: false, drag_off_x: 0, drag_off_y: 0,
    });

    let mut active_win: usize = 0;
    let mut drag_win: Option<usize> = None;
    let mut start_menu_open = false;
    let mut blink_timer: u32 = 0;

    let kbd_fd = CString::new("/dev/input/event0").ok()
        .map(|p| skyos_libc::syscall::open(p.as_ptr() as *const u8, 0))
        .unwrap_or(u64::MAX);
    let mouse_fd = CString::new("/dev/input/event1").ok()
        .map(|p| skyos_libc::syscall::open(p.as_ptr() as *const u8, 0))
        .unwrap_or(u64::MAX);

    loop {
        bb.clear(Color::BLACK);
        draw_desktop(&mut bb);
        for (i, win) in wins.iter().enumerate() {
            draw_window(&mut bb, win, i == active_win);
        }
        draw_taskbar(&mut bb, wins.len(), active_win);
        draw_start_menu(&mut bb, start_menu_open, mouse.x, mouse.y);
        draw_mouse_cursor(&mut bb, mouse.x, mouse.y);
        fb.blit(bb.as_bytes());

        let mut events = alloc::vec::Vec::new();
        let mut buf512 = [0u8; 512];

        if (kbd_fd as i64) >= 0 {
            let n = read_input_events(kbd_fd, &mut buf512);
            events.extend(parse_input_events(&buf512, n, &mut mouse));
        }
        if (mouse_fd as i64) >= 0 {
            let n = read_input_events(mouse_fd, &mut buf512);
            events.extend(parse_input_events(&buf512, n, &mut mouse));
        }

        if events.is_empty() {
            blink_timer += 1;
            if blink_timer > 10 {
                if let Some(w) = wins.get_mut(active_win) {
                    if let Some(term) = w.app.as_any_mut().downcast_mut::<SkyTerm>() {
                        term.cursor_blink = !term.cursor_blink;
                    }
                }
                blink_timer = 0;
            }
        }

        // Process accumulated events
        // Handle mouse events for window management first
        for &ev in &events {
            match ev {
                Event::MouseMove(mx, my) => {
                    mouse.x = mx;
                    mouse.y = my;
                    if let Some(drag_idx) = drag_win {
                        if let Some(w) = wins.get_mut(drag_idx) {
                            w.x = mx - w.drag_off_x;
                            w.y = my - w.drag_off_y;
                            w.x = core::cmp::max(0, core::cmp::min((SCREEN_W as i32 - w.w as i32), w.x));
                            w.y = core::cmp::max(0, core::cmp::min((SCREEN_H as i32 - TASKBAR_H as i32 - w.h as i32), w.y));
                        }
                    }
                }
                Event::KeyDown(code) => {
                    match code {
                        1 => return 0, // ESC
                        15 => { // TAB
                            if wins.len() > 1 {
                                active_win = (active_win + 1) % wins.len();
                            }
                        }
                        59 => { // F1 - add Terminal
                            let nid = wins.len();
                            wins.push(Win {
                                x: 80 + (nid as i32 * 30) % 400,
                                y: 80 + (nid as i32 * 30) % 300,
                                w: 520, h: 380,
                                app: Box::new(SkyTerm::new()),
                                dragging: false, drag_off_x: 0, drag_off_y: 0,
                            });
                            active_win = wins.len() - 1;
                        }
                        60 => { // F2 - add Calculator
                            let nid = wins.len();
                            wins.push(Win {
                                x: 100 + (nid as i32 * 30) % 400,
                                y: 100 + (nid as i32 * 30) % 300,
                                w: 320, h: 380,
                                app: Box::new(SkyCalc::new()),
                                dragging: false, drag_off_x: 0, drag_off_y: 0,
                            });
                            active_win = wins.len() - 1;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Now check for mouse button state changes
        // Read mouse events again for button state
        if (mouse_fd as i64) >= 0 {
            let mut buf256 = [0u8; 256];
            let n = read_input_events(mouse_fd, &mut buf256);
            let mut off = 0;
            while off + 8 <= n {
                let kind = u16::from_le_bytes([buf256[off], buf256[off + 1]]);
                let code = u16::from_le_bytes([buf256[off + 2], buf256[off + 3]]);
                let value = i32::from_le_bytes([buf256[off + 4], buf256[off + 5], buf256[off + 6], buf256[off + 7]]);
                off += 8;
                if kind == 1 && code == 272 { // EV_KEY + BTN_LEFT
                    let mx = mouse.x;
                    let my = mouse.y;
                    if value == 1 {
                        mouse.left = true;
                        // Check start menu
                        if start_menu_open && mx >= 0 && mx < 180 && my >= (SCREEN_H - TASKBAR_H) as i32 - 200 && my < (SCREEN_H - TASKBAR_H) as i32 {
                            let item_idx = (my - ((SCREEN_H - TASKBAR_H) as i32 - 200)) / 36;
                            match item_idx {
                                0 => {
                                    wins.push(Win {
                                        x: 100, y: 100, w: 520, h: 380,
                                        app: Box::new(SkyTerm::new()),
                                        dragging: false, drag_off_x: 0, drag_off_y: 0,
                                    });
                                    active_win = wins.len() - 1;
                                }
                                1 => {
                                    wins.push(Win {
                                        x: 200, y: 150, w: 320, h: 380,
                                        app: Box::new(SkyCalc::new()),
                                        dragging: false, drag_off_x: 0, drag_off_y: 0,
                                    });
                                    active_win = wins.len() - 1;
                                }
                                _ => {}
                            }
                            start_menu_open = false;
                        } else if my >= (SCREEN_H - TASKBAR_H) as i32 {
                            // Click on desktop / taskbar area
                            if mx >= 0 && mx < 80 {
                                start_menu_open = !start_menu_open;
                            } else {
                                start_menu_open = false;
                                // Check taskbar buttons
                                for i in 0..wins.len() {
                                    let btn_x = 8 + i as i32 * 120;
                                    if mx >= btn_x && mx < btn_x + 110 {
                                        active_win = i;
                                        break;
                                    }
                                }
                            }
                        } else {
                            start_menu_open = false;
                            if let Some(idx) = zorder_test(&wins, mx, my) {
                                active_win = idx;
                                if hit_titlebar(&wins[idx], mx, my) {
                                    drag_win = Some(idx);
                                    wins[idx].drag_off_x = mx - wins[idx].x;
                                    wins[idx].drag_off_y = my - wins[idx].y;
                                    wins[idx].dragging = true;
                                }
                                // Forward click to app
                                let app_ev = Event::MouseDown(mx, my, 272);
                                if let Some(w) = wins.get_mut(idx) {
                                    let cx = w.x as usize + 2;
                                    let cy = w.y as usize + TITLEBAR_H + 2;
                                    let cw = w.w - 4;
                                    let ch = w.h - TITLEBAR_H - 4;
                                    w.app.handle_event(app_ev, cx, cy, cw, ch);
                                }
                            }
                        }
                    } else if value == 0 {
                        mouse.left = false;
                        if let Some(idx) = drag_win {
                            wins[idx].dragging = false;
                            drag_win = None;
                        }
                    }
                }
            }
        }

        // Forward keyboard events to active window
        for ev in &events {
            match ev {
                Event::KeyDown(code) => {
                    match *code {
                        1 | 15 | 59 | 60 => {} // already handled
                        _ => {
                            if let Some(w) = wins.get_mut(active_win) {
                                let cx = w.x as usize + 2;
                                let cy = w.y as usize + TITLEBAR_H + 2;
                                w.app.handle_event(*ev, cx, cy, w.w - 4, w.h - TITLEBAR_H - 4);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let _ = skyos_libc::syscall::syscall1(
            skyos_libc::SYS_NANOSLEEP,
            33_000_000,
        );
    }
}

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
