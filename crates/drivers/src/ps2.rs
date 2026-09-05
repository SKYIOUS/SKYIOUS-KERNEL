// SAFETY CONTRACT:
// PS/2 controller registers are accessed via I/O ports 0x60 (data) and
// 0x64 (status/command). Port reads check status register bit 0 (output
// buffer full) before reading data. IRQ handlers drain the buffer in a
// loop until empty. All ports are fixed x86 PS/2 hardware addresses.
use core::sync::atomic::{AtomicU64, Ordering};
use vahi_sync::IrqSafeMutex as Mutex;
use x86_64::instructions::port::Port;

static PS2_LOCK: Mutex<()> = Mutex::new(());
pub static KBD_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);
static MOUSE_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);

/// Return (keyboard_irq_count, mouse_irq_count) for /proc/interrupts.
pub fn irq_counts() -> (u64, u64) {
    (
        KBD_IRQ_COUNT.load(Ordering::Relaxed),
        MOUSE_IRQ_COUNT.load(Ordering::Relaxed),
    )
}

fn wait_write() {
    let mut status = Port::<u8>::new(0x64);
    for _ in 0..100000 {
        unsafe {
            if status.read() & 2 == 0 {
                return;
            }
        }
        core::hint::spin_loop();
    }
}

fn wait_read() {
    let mut status = Port::<u8>::new(0x64);
    for _ in 0..100000 {
        unsafe {
            if status.read() & 1 != 0 {
                return;
            }
        }
        core::hint::spin_loop();
    }
}

fn write_command(cmd: u8) {
    let mut port = Port::<u8>::new(0x64);
    wait_write();
    unsafe {
        port.write(cmd);
    }
}

fn write_data(data: u8) {
    let mut port = Port::<u8>::new(0x60);
    wait_write();
    unsafe {
        port.write(data);
    }
}

fn read_data() -> u8 {
    let mut port = Port::<u8>::new(0x60);
    wait_read();
    unsafe { port.read() }
}

fn read_config() -> u8 {
    write_command(0x20);
    read_data()
}

fn write_config(value: u8) {
    write_command(0x60);
    write_data(value);
}

fn device_write_to_keyboard(data: u8) -> u8 {
    write_data(data);
    read_data()
}

fn device_write_to_mouse(data: u8) -> u8 {
    write_command(0xD4);
    write_data(data);
    read_data()
}

pub fn init() {
    // ponytail: ignore ACPI 8042 presence flag — QEMU doesn't report it but the controller works

    let _lock = PS2_LOCK.lock();

    // 1. Disable devices
    write_command(0xAD);
    write_command(0xA7);

    // 2. Flush output buffer
    {
        let mut status = Port::<u8>::new(0x64);
        for _ in 0..100 {
            unsafe {
                if status.read() & 1 != 0 {
                    Port::<u8>::new(0x60).read();
                } else {
                    break;
                }
            }
        }
    }

    // 3. Read and update config byte: enable both interrupts + enable clocks
    let config = read_config();
    let new_config = config | 0x03; // Bit 0 = Kbd IRQ enable, Bit 1 = Mouse IRQ enable
    write_config(new_config);

    // 4. Controller self-test
    write_command(0xAA);
    if read_data() != 0x55 {
        // println removed during extraction
    }

    // Re-write config byte after self-test — some 8042 implementations
    // reset the config byte during self-test, clearing IRQ enables.
    write_config(new_config);

    // 5. Enable devices
    write_command(0xAE); // Enable keyboard
    write_command(0xA8); // Enable mouse (aux)

    // 6. Set keyboard defaults and enable scanning
    let ack = device_write_to_keyboard(0xFF); // Reset
    crate::serial_write("");
    if ack == 0xFA || ack == 0xAA {
        crate::serial_write("");
        let bat = read_data();
        crate::serial_write("");
    }

    let ack = device_write_to_keyboard(0xF6); // Set defaults
    crate::serial_write("");

    let ack = device_write_to_keyboard(0xF4); // Enable scanning
    crate::serial_write("");

    // 7. Set mouse defaults and enable streaming
    let ack = device_write_to_mouse(0xFF); // Reset
    crate::serial_write("");
    if ack == 0xFA || ack == 0xAA {
        let bat = read_data();
        crate::serial_write("");
        // Mouse sends device ID (0x00 for standard) after BAT — consume it
        let dev_id = read_data();
        crate::serial_write("");
    }

    let ack = device_write_to_mouse(0xF6); // Set defaults
    crate::serial_write("");

    // Enable scroll wheel (IntelliMouse magic sequence)
    let _ = device_write_to_mouse(0xF3); // Set sample rate command
    let _ = device_write_to_mouse(200); // Sample rate value 200
    let _ = device_write_to_mouse(0xF3); // Set sample rate command
    let _ = device_write_to_mouse(100); // Sample rate value 100
    let _ = device_write_to_mouse(0xF3); // Set sample rate command
    let _ = device_write_to_mouse(80); // Sample rate value 80

    // Read device ID — 3 or 4 means wheel present
    let _ = device_write_to_mouse(0xF2); // Read device ID
    let dev_id = read_data();
    crate::serial_write("");
    if dev_id == 3 || dev_id == 4 {
        crate::serial_write("");
        crate::mouse::enable_wheel();
    }

    crate::serial_write("");
    let ack = device_write_to_mouse(0xF4); // Enable streaming
    crate::serial_write("");

    // Flush any stale bytes remaining in the output buffer
    {
        let mut status = Port::<u8>::new(0x64);
        let mut data_port = Port::<u8>::new(0x60);
        for _ in 0..16 {
            unsafe {
                if status.read() & 1 != 0 {
                    data_port.read();
                } else {
                    break;
                }
            }
        }
    }

    // Enable PS/2 IRQs via legacy PIC (IOAPIC delivery broken)
    // Directly write PIC masks using port I/O — no lock, no function call
    unsafe {
        // Master PIC data port 0x21: unmask IRQ1 (kbd) + IRQ2 (cascade)
        let mut master_mask = Port::<u8>::new(0x21);
        let m = master_mask.read() & 0xF9;
        master_mask.write(m);
        // Slave PIC data port 0xA1: unmask IRQ4 (= global IRQ12 mouse)
        let mut slave_mask = Port::<u8>::new(0xA1);
        let s = slave_mask.read() & 0xEF;
        slave_mask.write(s);
        crate::serial_write("");
    }

    // println removed during extraction
}
