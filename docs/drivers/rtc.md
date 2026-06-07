# Real-Time Clock Driver

The Real-Time Clock (RTC) driver provides battery-backed wall-clock time and alarm functionality.

## Hardware Interface

The RTC is accessed through CMOS memory at I/O ports 0x70 (index) and 0x71 (data). The driver reads time information as binary-coded decimal (BCD) values and converts them to integer format.

```rust
pub struct RtcTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u16,
    pub century: Option<u8>,
}
```

## Time Reading

The driver reads all time fields in a loop until two consecutive reads match, ensuring a consistent time snapshot:

```rust
pub fn read_rtc_time() -> RtcTime {
    loop {
        let first = read_time_fields();
        let second = read_time_fields();
        if first == second {
            return first;
        }
    }
}
```

## RTC Configuration

The driver configures the RTC to:
- Use binary format (not BCD)
- Enable periodic interrupts (optional)
- Set the century register if available
- Configure alarm registers (if used)

## Alarm Support

The RTC alarm can trigger an interrupt at a specified time. This is used for wake-from-sleep functionality. Alarm registers store the target time; when the RTC matches, it raises IRQ 8.

## NTP Integration

The RTC time is used as the initial time source during boot. After the network stack is available, NTP provides more accurate time, and the RTC is updated periodically to maintain time across reboots.
