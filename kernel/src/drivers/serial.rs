use core::sync::atomic::{AtomicBool, AtomicU16};
use x86_64::instructions::port::Port;

const COM1: u16 = 0x3F8;

const DATA: u16 = 0;
const INTR_EN: u16 = 1;
const FIFO_CTRL: u16 = 2;
const LINE_CTRL: u16 = 3;
const MODEM_CTRL: u16 = 4;
const LINE_STATUS: u16 = 5;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static SERIAL_PORT: AtomicU16 = AtomicU16::new(COM1);

pub fn init(port: u16) {
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
        line.write(0x80);

        // Baud rate divisor: 115200 / 9600 = 12
        let mut data = Port::<u8>::new(base + DATA);
        let mut intr2 = Port::<u8>::new(base + INTR_EN);
        data.write(0x0C); // 9600 baud low byte
        intr2.write(0x00); // 9600 baud high byte

        // 8 bits, no parity, 1 stop bit
        line.write(0x03);

        // Enable FIFO, clear TX/RX, 14-byte threshold
        fifo.write(0xC7);

        // Enable DTR, RTS, OUT2
        modem.write(0x0B);
    }
    INITIALIZED.store(true, core::sync::atomic::Ordering::SeqCst);
}

fn com_port() -> u16 {
    SERIAL_PORT.load(core::sync::atomic::Ordering::Acquire)
}

pub fn putc(c: u8) {
    if !INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let port = com_port();
    let mut lsr = Port::<u8>::new(port + LINE_STATUS);
    let mut data = Port::<u8>::new(port + DATA);
    unsafe {
        while lsr.read() & 0x20 == 0 {}
        data.write(c);
    }
}

pub fn getc() -> Option<u8> {
    if !is_received() {
        return None;
    }
    let port = com_port();
    let mut data = Port::<u8>::new(port + DATA);
    Some(unsafe { data.read() })
}

pub fn is_received() -> bool {
    if !INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let port = com_port();
    let mut lsr = Port::<u8>::new(port + LINE_STATUS);
    unsafe { lsr.read() & 1 != 0 }
}

pub fn write_str(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            putc(b'\r');
        }
        putc(b);
    }
}

pub fn is_initialized() -> bool {
    INITIALIZED.load(core::sync::atomic::Ordering::Relaxed)
}
