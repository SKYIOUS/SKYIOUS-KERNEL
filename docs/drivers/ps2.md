# PS/2 Controller Driver

The PS/2 controller driver manages the legacy PS/2 interface for keyboard and mouse input.

## Controller Initialization

The PS/2 controller is initialized during boot:

1. Disable both PS/2 ports
2. Flush the output buffer
3. Perform controller self-test (0xAA)
4. Set configuration byte (enable interrupts, set clock rates)
5. Enable ports and configure devices

```rust
pub fn init_ps2_controller() -> Result<Ps2Controller, DriverError> {
    // Disable devices
    write_command(0xAD); // Disable port 1
    write_command(0xA7); // Disable port 2
    
    // Self test
    write_command(0xAA);
    let result = read_data();
    if result != 0x55 {
        return Err(DriverError::InitFailed);
    }
    
    // Enable and configure
    write_command(0x60); // Write configuration byte
    write_data(0x47);    // Enable IRQs, set clock
    Ok(Ps2Controller::new())
}
```

## Port Access

The controller uses I/O ports 0x60 (data) and 0x64 (command/status). The status register indicates when data is available or the controller is ready to accept commands.

## Interrupts

Port 1 (keyboard) uses IRQ 1, Port 2 (mouse) uses IRQ 12. Both are edge-triggered. The interrupt handlers read the scancode or mouse data from port 0x60 and queue it for processing.

## Dual-Channel Support

The controller supports two channels. Channel 1 is always present; channel 2 is optional. Detection is done by attempting to enable channel 2 and checking the configuration byte.
