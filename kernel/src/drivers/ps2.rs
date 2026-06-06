use x86_64::instructions::port::Port;
use spin::Mutex;

static PS2_LOCK: Mutex<()> = Mutex::new(());

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
    unsafe { port.write(cmd); }
}

fn write_data(data: u8) {
    let mut port = Port::<u8>::new(0x60);
    wait_write();
    unsafe { port.write(data); }
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
    let new_config = config | 0x03;  // Bit 0 = Kbd IRQ enable, Bit 1 = Mouse IRQ enable
    write_config(new_config);

    // 4. Controller self-test
    write_command(0xAA);
    if read_data() != 0x55 {
        crate::println!("PS/2: Self-test failed!");
    }

    // 5. Enable devices
    write_command(0xAE); // Enable keyboard
    write_command(0xA8); // Enable mouse (aux)

    // 6. Set keyboard defaults and enable scanning
    let ack = device_write_to_keyboard(0xFF); // Reset
    crate::serial_write(&alloc::format!("[PS2] kbd reset ack=0x{:x}\n", ack));
    if ack == 0xFA || ack == 0xAA {
        crate::serial_write("[PS2] kbd reset OK (waiting bat...)\n");
        let bat = read_data();
        crate::serial_write(&alloc::format!("[PS2] kbd bat=0x{:x}\n", bat));
    }

    let ack = device_write_to_keyboard(0xF6); // Set defaults
    crate::serial_write(&alloc::format!("[PS2] kbd set_defaults ack=0x{:x}\n", ack));

    let ack = device_write_to_keyboard(0xF4); // Enable scanning
    crate::serial_write(&alloc::format!("[PS2] kbd enable_scan ack=0x{:x}\n", ack));

    // 7. Set mouse defaults and enable streaming
    let ack = device_write_to_mouse(0xFF); // Reset
    crate::serial_write(&alloc::format!("[PS2] mouse reset ack=0x{:x}\n", ack));
    if ack == 0xFA || ack == 0xAA {
        let bat = read_data();
        crate::serial_write(&alloc::format!("[PS2] mouse bat=0x{:x}\n", bat));
        // Mouse sends device ID (0x00 for standard) after BAT — consume it
        let dev_id = read_data();
        crate::serial_write(&alloc::format!("[PS2] mouse device_id=0x{:x}\n", dev_id));
    }

    let ack = device_write_to_mouse(0xF6); // Set defaults
    crate::serial_write(&alloc::format!("[PS2] mouse set_defaults ack=0x{:x}\n", ack));

    // Enable scroll wheel (IntelliMouse magic sequence)
    let _ = device_write_to_mouse(0xF3); // Set sample rate command
    let _ = device_write_to_mouse(200);  // Sample rate value 200
    let _ = device_write_to_mouse(0xF3); // Set sample rate command
    let _ = device_write_to_mouse(100);  // Sample rate value 100
    let _ = device_write_to_mouse(0xF3); // Set sample rate command
    let _ = device_write_to_mouse(80);   // Sample rate value 80

    // Read device ID — 3 or 4 means wheel present
    let _ = device_write_to_mouse(0xF2); // Read device ID
    let dev_id = read_data();
    crate::serial_write(&alloc::format!("[PS2] mouse wheel_dev_id=0x{:x}\n", dev_id));
    if dev_id == 3 || dev_id == 4 {
        crate::serial_write("[PS2] mouse scroll wheel detected!\n");
        crate::drivers::mouse::enable_wheel();
    }

    let ack = device_write_to_mouse(0xF4); // Enable streaming
    crate::serial_write(&alloc::format!("[PS2] mouse enable_stream ack=0x{:x}\n", ack));

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

    crate::println!("PS/2 Controller and Devices Initialized");
}
