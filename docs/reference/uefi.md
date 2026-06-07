# UEFI Boot Process Reference

SkyOS boots through the UEFI firmware interface.

## Boot Phases

1. **SEC** (Security Phase): Initial platform initialization
2. **PEI** (Pre-EFI Initialization): Memory initialization
3. **DXE** (Driver Execution Environment): UEFI driver loading
4. **BDS** (Boot Device Selection): Chooses the boot device
5. **TSL** (Transient System Load): Bootloader execution
6. **RT** (Runtime): OS running with UEFI runtime services

## UEFI Services Used

### Boot Services (terminated before kernel entry)

| Service | Purpose |
|---------|---------|
| `ExitBootServices` | Terminate UEFI boot services |
| `GetMemoryMap` | Retrieve physical memory map |
| `AllocatePages` | Allocate physical memory |
| `LocateProtocol` | Find UEFI protocols |
| `SetWatchdogTimer` | Watchdog control |

### Runtime Services (available after boot)

| Service | Purpose |
|---------|---------|
| `GetVariable` | Read UEFI variables |
| `SetVariable` | Write UEFI variables |
| `GetTime` | Read real-time clock |
| `SetTime` | Set real-time clock |
| `ResetSystem` | System reset or shutdown |

## Protocols Used

| Protocol | Purpose |
|----------|---------|
| Graphics Output Protocol (GOP) | Framebuffer for display |
| Simple File System Protocol | Reading kernel from disk |
| Loaded Image Protocol | Bootloader image info |
| Device Path Protocol | Boot device path |

## Memory Map

The memory map from `GetMemoryMap` includes:
- Conventional memory (usable by OS)
- Boot services code/data (usable after ExitBootServices)
- Runtime services code/data (preserved)
- ACPI NVS and reclaimable memory
- Reserved and MMIO regions

## UEFI Variables

SkyOS reads these UEFI variables during boot:
- `BootOrder` - Boot device order
- `SecureBoot` - Secure boot status
- `SetupMode` - Setup mode indicator
