#![no_std]

extern crate alloc;

/// Play a beep using the PC speaker via kernel SYS_BEEP
pub fn beep(freq_hz: u32, duration_ms: u32) {
    skyos_libc::syscall::beep(freq_hz, duration_ms);
}

/// Play a beep via /dev/speaker (write 8 bytes: freq+duration as u32 LE)
pub fn beep_dev(freq_hz: u32, duration_ms: u32) -> bool {
    let fd = skyos_libc::syscall::open("/dev/speaker\0".as_ptr() as *const u8, 1); // O_WRONLY
    if (fd as i64) < 0 { return false; }
    let data: [u8; 8] = [
        freq_hz as u8,
        (freq_hz >> 8) as u8,
        (freq_hz >> 16) as u8,
        (freq_hz >> 24) as u8,
        duration_ms as u8,
        (duration_ms >> 8) as u8,
        (duration_ms >> 16) as u8,
        (duration_ms >> 24) as u8,
    ];
    let ret = skyos_libc::syscall::write(fd, &data);
    skyos_libc::syscall::close(fd);
    ret == 8
}
