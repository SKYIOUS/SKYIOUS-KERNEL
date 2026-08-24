use core::sync::atomic::{AtomicBool, AtomicU16};
use x86_64::instructions::port::Port;

/// Serial port I/O register offsets
const DATA: u16 = 0;
const INTR_EN: u16 = 1;
const FIFO_CTRL: u16 = 2;
const LINE_CTRL: u16 = 3;
const MODEM_CTRL: u16 = 4;
const LINE_STATUS: u16 = 5;

/// Serial port configuration constants
const COM1: u16 = 0x3F8;
const BAUD_115200_LOW: u8 = 0x01;
const BAUD_115200_HIGH: u8 = 0x00;
const DLAB_ENABLE: u8 = 0x80;
const LINE_CONFIG_8N1: u8 = 0x03;
const FIFO_CONFIG: u8 = 0xC7;
const MODEM_CONFIG: u8 = 0x0B;
const TX_READY: u8 = 0x20;
const RX_READY: u8 = 0x01;
/// ~1ms of port reads at typical emulation speed; real 16550s never hit this.
const TX_TIMEOUT_SPINS: u32 = 1_000_000;

static TX_DROPPED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static SERIAL_PORT: AtomicU16 = AtomicU16::new(COM1);

/// Initialize serial port with error handling
/// Returns Ok(()) on success, Err(()) on failure
pub fn init(port: u16) -> Result<(), ()> {
    let base = port;
    SERIAL_PORT.store(port, core::sync::atomic::Ordering::Release);
    unsafe {
        let mut intr = Port::<u8>::new(base + INTR_EN);
        let mut line = Port::<u8>::new(base + LINE_CTRL);
        let mut fifo = Port::<u8>::new(base + FIFO_CTRL);
        let mut modem = Port::<u8>::new(base + MODEM_CTRL);

        // Disable interrupts
        intr.write(0x00);

        // Set DLAB to configure baud rate
        line.write(DLAB_ENABLE);

        // Baud rate divisor: 115200 / 115200 = 1
        let mut data = Port::<u8>::new(base + DATA);
        let mut intr2 = Port::<u8>::new(base + INTR_EN);
        data.write(BAUD_115200_LOW);
        intr2.write(BAUD_115200_HIGH);

        // 8 bits, no parity, 1 stop bit
        line.write(LINE_CONFIG_8N1);

        // Enable FIFO, clear TX/RX, 14-byte threshold
        fifo.write(FIFO_CONFIG);

        // Enable DTR, RTS, OUT2
        modem.write(MODEM_CONFIG);
    }
    INITIALIZED.store(true, core::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Cleanup serial port - disable interrupts and reset
pub fn cleanup() {
    if !INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let port = com_port();
    unsafe {
        let mut intr = Port::<u8>::new(port + INTR_EN);
        let mut fifo = Port::<u8>::new(port + FIFO_CTRL);
        // Disable interrupts
        intr.write(0x00);
        // Disable FIFO
        fifo.write(0x00);
    }
    INITIALIZED.store(false, core::sync::atomic::Ordering::SeqCst);
}

fn com_port() -> u16 {
    SERIAL_PORT.load(core::sync::atomic::Ordering::Acquire)
}

/// Write a single character to the serial port
// ponytail: bounded LSR spin — a wedged QEMU pipe/backend gateways a
// real UART into freeze-forever. On timeout the byte is dropped; the
// next healthy tick shows the drop counter. Upgrade to IRQ-driven TX if
// real hardware ever wedges its 16550.
pub fn putc(c: u8) {
    if !INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let port = com_port();
    let mut lsr = Port::<u8>::new(port + LINE_STATUS);
    let mut data = Port::<u8>::new(port + DATA);
    unsafe {
        let mut spins = 0u32;
        while lsr.read() & TX_READY == 0 {
            spins += 1;
            if spins > TX_TIMEOUT_SPINS {
                TX_DROPPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                return;
            }
        }
        data.write(c);
    }
}

/// Number of putc bytes dropped on TX stall
pub fn tx_dropped() -> u32 {
    TX_DROPPED.load(core::sync::atomic::Ordering::Relaxed)
}

/// Read a single character from the serial port
pub fn getc() -> Option<u8> {
    if !is_received() {
        return None;
    }
    let port = com_port();
    let mut data = Port::<u8>::new(port + DATA);
    Some(unsafe { data.read() })
}

/// Check if data is available to read
pub fn is_received() -> bool {
    if !INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let port = com_port();
    let mut lsr = Port::<u8>::new(port + LINE_STATUS);
    unsafe { lsr.read() & RX_READY != 0 }
}

/// Write a string to the serial port
pub fn write_str(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            putc(b'\r');
        }
        putc(b);
    }
}

/// Check if serial port is initialized
pub fn is_initialized() -> bool {
    INITIALIZED.load(core::sync::atomic::Ordering::Relaxed)
}
