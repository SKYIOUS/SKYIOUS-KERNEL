# Mouse Driver

The PS/2 mouse driver handles relative motion input from standard PS/2 mice.

## Initialization

The mouse is initialized after the PS/2 controller is ready:

```rust
pub fn init_mouse(controller: &mut Ps2Controller) -> Result<MouseDevice, DriverError> {
    // Enable mouse on PS/2 port 2
    controller.write_command(0xA8);
    
    // Set default settings
    controller.write_to_mouse(0xF6); // Set defaults
    controller.read_from_mouse()?;   // ACK
    
    // Enable data reporting
    controller.write_to_mouse(0xF4);
    controller.read_from_mouse()?;   // ACK
    
    // Enable scroll wheel (IntelliMouse protocol)
    controller.write_to_mouse(0xF3); // Set sample rate
    controller.read_from_mouse()?;
    controller.write_to_mouse(200);  // 200 samples/sec
    controller.read_from_mouse()?;
    
    Ok(MouseDevice::new())
}
```

## Data Packet Format

Standard PS/2 mouse uses 3-byte packets:

| Byte | Bits | Description |
|------|------|-------------|
| 0 | 0 | Left button |
| 0 | 1 | Right button |
| 0 | 2 | Middle button |
| 0 | 4 | X sign (9th bit) |
| 0 | 5 | Y sign (9th bit) |
| 0 | 6 | X overflow |
| 0 | 7 | Y overflow |
| 1 | 0-7 | X movement delta |
| 2 | 0-7 | Y movement delta |

IntelliMouse protocol adds a 4th byte for scroll wheel data.

## Event Processing

The mouse driver converts raw packets into motion events:
- Accumulates delta X/Y values
- Tracks button state (press/release)
- Reports scroll wheel movement
- Handles overflow conditions by discarding large deltas
