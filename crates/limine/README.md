# vahi-limine

Limine boot protocol integration — defines static Limine requests and
provides accessor functions for boot information.

## Accessor Functions

| Function | Returns |
|----------|---------|
| `hhdm_offset()` | Physical→Virtual offset |
| `memory_map()` | Usable/reserved memory regions |
| `framebuffer()` | Linear framebuffer info |
| `rsdp_addr()` | ACPI RSDP physical address |
| `ramdisk()` | Initrd/initramfs data |
| `max_physical_address()` | Top of physical memory |
| `iter_usable_regions()` | Usable (base, end) pairs |

## Limine Protocol Markers

All request statics use `#[link_section = ".limine_requests"]`.
The linker script enforces ordering: start → requests → end.
`prevent_stripping()` prevents LTO from removing the markers.

## Dependencies

- `limine` crate 0.6+
