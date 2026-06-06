use x86_64::instructions::port::Port;

const PIT_FREQUENCY: u32 = 1193182;

pub fn beep(freq_hz: u32, duration_ms: u32) {
    if freq_hz == 0 {
        return;
    }

    let divisor = (PIT_FREQUENCY / freq_hz) as u16;

    // Set PIT channel 2 to mode 3 (square wave generator)
    let mut pit_cmd: Port<u8> = Port::new(0x43);
    unsafe {
        pit_cmd.write(0xB6u8); // Channel 2, lo/hi, mode 3, binary
    }

    // Write frequency divisor to channel 2 data port
    let mut pit_data: Port<u8> = Port::new(0x42);
    unsafe {
        pit_data.write((divisor & 0xFF) as u8);
        pit_data.write((divisor >> 8) as u8);
    }

    // Enable speaker (set bits 0 and 1 of port 0x61)
    let mut speaker: Port<u8> = Port::new(0x61);
    unsafe {
        let tmp = speaker.read();
        speaker.write(tmp | 0x03);
    }

    // Busy-wait for the duration
    if duration_ms > 0 {
        for _ in 0..duration_ms {
            for _ in 0..100000 {
                core::hint::spin_loop();
            }
        }
    }

    // Disable speaker (clear bits 0 and 1)
    unsafe {
        let tmp = speaker.read();
        speaker.write(tmp & !0x03);
    }
}
