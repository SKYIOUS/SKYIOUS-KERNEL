use x86_64::instructions::port::Port;
use core::sync::atomic::{AtomicBool, Ordering};

static RTC_INITIALIZED: AtomicBool = AtomicBool::new(false);
static RTC_EPOCH_SECS: spin::Mutex<u64> = spin::Mutex::new(0);

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

fn cmos_read(reg: u8) -> u8 {
    let mut addr: Port<u8> = Port::new(CMOS_ADDR);
    let mut data: Port<u8> = Port::new(CMOS_DATA);
    unsafe {
        addr.write(reg | 0x80);
        data.read()
    }
}

fn is_updating() -> bool {
    cmos_read(0x0A) & 0x80 != 0
}

fn bcd_to_binary(bcd: u8) -> u8 {
    (bcd & 0x0F) + ((bcd >> 4) * 10)
}

fn cmos_read_time() -> (u64, u64) {
    while is_updating() {}
    let second = bcd_to_binary(cmos_read(0x00));
    let minute = bcd_to_binary(cmos_read(0x02));
    let hour = bcd_to_binary(cmos_read(0x04));
    let day = bcd_to_binary(cmos_read(0x07));
    let month = bcd_to_binary(cmos_read(0x08));
    let year = bcd_to_binary(cmos_read(0x09));
    while is_updating() {}

    let year_full = 2000u64 + year as u64;

    let days_since_epoch = days_from_ymd(year_full, month as u64, day as u64);
    let total_secs = days_since_epoch * 86400 + (hour as u64) * 3600 + (minute as u64) * 60 + second as u64;
    (total_secs, 0)
}

fn days_from_ymd(year: u64, month: u64, day: u64) -> u64 {
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 12 } else { month };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m - 3) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe
}

pub fn init() {
    let (secs, _) = cmos_read_time();
    let mut epoch = RTC_EPOCH_SECS.lock();
    *epoch = secs;
    RTC_INITIALIZED.store(true, Ordering::SeqCst);
}

pub fn read_realtime() -> (i64, i64) {
    if !RTC_INITIALIZED.load(Ordering::SeqCst) {
        return (0, 0);
    }
    let epoch_base = *RTC_EPOCH_SECS.lock();
    let ticks = crate::interrupts::get_ticks();
    let elapsed_ms = ticks * 10;
    let total_secs = epoch_base + (elapsed_ms / 1000);
    let remaining_ns = (elapsed_ms % 1000) * 1_000_000;
    (total_secs as i64, remaining_ns as i64)
}
