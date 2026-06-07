# Keyboard Driver

The PS/2 keyboard driver handles scancode translation and input event generation.

## Scancode Sets

The driver supports scancode set 1 (default after reset). Each key press generates a make code, and each release generates a break code (0x80 + make code).

```rust
pub enum KeyEvent {
    Pressed { key: KeyCode, modifiers: ModifierState },
    Released { key: KeyCode, modifiers: ModifierState },
}
```

## Modifier Tracking

The driver maintains modifier state:

```rust
pub struct ModifierState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub gui: bool,  // Windows/Command key
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}
```

## Keyboard Layout

The driver includes a basic US QWERTY layout. Layout support is implemented through a keymap table that maps scancodes to character values based on modifier state. Future versions will support loadable keymap files.

## Special Keys

Extended scancodes (prefixed with 0xE0) are handled for:
- Arrow keys
- Home/End/Page Up/Page Down
- Insert/Delete
- Application keys (menu, sleep, power)
- Multimedia keys (volume, play/pause)

## Typematic Rate

The driver configures the keyboard's typematic delay (0.5s) and repeat rate (30 characters/second). These can be adjusted through the `ioctl()` syscall.

## LED Synchronization

The driver updates keyboard LEDs (Caps Lock, Num Lock, Scroll Lock) by sending the `0xED` command with the LED state byte whenever the modifier state changes.
