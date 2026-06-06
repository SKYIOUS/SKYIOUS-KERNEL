#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use skyos_libc::syscall::{write, exit, open, close, getdents64};
use alloc::ffi::CString;
use alloc::vec::Vec;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { exit(1); }

const PCI_DEVS: &[u8] = b"/sys/bus/pci/devices/\0";
const VENDORS: &[(u16, &str, &[(u16, &str)])] = &[
    (0x8086, "Intel Corporation", &[
        (0x29c0, "DRAM Controller"),
        (0x2918, "LPC Interface Controller"),
        (0x2922, "SATA AHCI Controller"),
        (0x2930, "SMBus Controller"),
        (0x2934, "High Definition Audio"),
        (0x2940, "USB UHCI Controller #1"),
        (0x2942, "USB UHCI Controller #2"),
        (0x2944, "USB UHCI Controller #3"),
        (0x2946, "USB UHCI Controller #4"),
        (0x2948, "USB UHCI Controller #5"),
        (0x294a, "USB UHCI Controller #6"),
        (0x294c, "USB EHCI Controller #1"),
        (0x294e, "USB EHCI Controller #2"),
        (0x2972, "HD Graphics"),
    ]),
    (0x1022, "AMD Inc.", &[
        (0x43b4, "X399 Series Chipset"),
        (0x43b5, "USB 3.0 Host Controller"),
        (0x43b6, "SATA Controller"),
        (0x43b7, "Audio Device"),
    ]),
    (0x10de, "NVIDIA Corporation", &[
        (0x1a03, "GP102 GPU"),
        (0x2204, "GA102 GPU"),
        (0x13c2, "Geforce GTX 970"),
    ]),
    (0x1af4, "Red Hat / QEMU", &[
        (0x1000, "Virtio Network Device"),
        (0x1001, "Virtio Block Device"),
        (0x1002, "Virtio Console"),
        (0x1003, "Virtio RNG"),
        (0x1004, "Virtio Memory Balloon"),
        (0x1005, "Virtio GPU"),
        (0x1009, "Virtio 9P Filesystem"),
        (0x1042, "Virtio SCSI"),
        (0x1045, "Virtio Input"),
    ]),
    (0x1234, "Bochs/QEMU VGA", &[(0x1111, "VGA Compatible Controller")]),
    (0x15ad, "VMware Inc.", &[
        (0x0405, "SVM SVGA II Adapter"),
        (0x0790, "PCI Express Root Port"),
    ]),
    (0x1002, "Advanced Micro Devices", &[
        (0x1636, "Raven Ridge GPU"),
        (0x15d8, "Picasso GPU"),
    ]),
    (0x103c, "Hewlett-Packard", &[]),
    (0x104c, "Texas Instruments", &[]),
    (0x10ec, "Realtek Semiconductor", &[(0x8168, "RTL8111 Gigabit Ethernet")]),
    (0x8087, "Intel Bluetooth", &[]),
    (0x8088, "Intel (Redundant)", &[]),
    (0x1b36, "Red Hat / QEMU (PCI)", &[(0x000d, "QEMU XHCI Host Controller")]),
];

fn vendor_name(vid: u16) -> &'static str {
    for (v, name, _) in VENDORS { if *v == vid { return name; } }
    "Unknown Vendor"
}

fn device_name(vid: u16, did: u16) -> &'static str {
    for (v, _, devices) in VENDORS {
        if *v == vid {
            for (d, name) in *devices { if *d == did { return name; } }
        }
    }
    "Unknown Device"
}

fn class_name(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x00, 0x00) => "Unclassified (Old)",
        (0x01, 0x00) => "Mass Storage: SCSI",
        (0x01, 0x01) => "Mass Storage: IDE",
        (0x01, 0x02) => "Mass Storage: Floppy",
        (0x01, 0x03) => "Mass Storage: IPI",
        (0x01, 0x04) => "Mass Storage: RAID",
        (0x01, 0x05) => "Mass Storage: ATA",
        (0x01, 0x06) => "Mass Storage: SATA",
        (0x01, 0x07) => "Mass Storage: SAS",
        (0x01, 0x08) => "Mass Storage: NVM",
        (0x02, 0x00) => "Network: Ethernet",
        (0x02, 0x01) => "Network: Token Ring",
        (0x02, 0x02) => "Network: FDDI",
        (0x02, 0x03) => "Network: ATM",
        (0x02, 0x04) => "Network: ISDN",
        (0x02, 0x05) => "Network: WorldFip",
        (0x02, 0x06) => "Network: PICMG",
        (0x03, 0x00) => "Display: VGA",
        (0x03, 0x01) => "Display: XGA",
        (0x03, 0x02) => "Display: 3D",
        (0x04, 0x00) => "Multimedia: Video",
        (0x04, 0x01) => "Multimedia: Audio",
        (0x04, 0x02) => "Multimedia: Phone",
        (0x04, 0x03) => "Multimedia: Audio Device",
        (0x05, 0x00) => "Memory: RAM",
        (0x05, 0x01) => "Memory: Flash",
        (0x06, 0x00) => "Bridge: Host",
        (0x06, 0x01) => "Bridge: ISA",
        (0x06, 0x02) => "Bridge: EISA",
        (0x06, 0x03) => "Bridge: PCI",
        (0x06, 0x04) => "Bridge: PCMCIA",
        (0x06, 0x05) => "Bridge: MCA",
        (0x06, 0x06) => "Bridge: PCI- PCI",
        (0x06, 0x07) => "Bridge: CardBus",
        (0x06, 0x08) => "Bridge: RACEway",
        (0x06, 0x09) => "Bridge: Semi-Transparent",
        (0x06, 0x0a) => "Bridge: InfiniBand",
        (0x07, 0x00) => "Serial: 8250 UART",
        (0x07, 0x01) => "Serial: 16450 UART",
        (0x07, 0x02) => "Serial: 16550 UART",
        (0x07, 0x03) => "Serial: 16650 UART",
        (0x07, 0x04) => "Serial: 16750 UART",
        (0x07, 0x05) => "Serial: 16850 UART",
        (0x07, 0x06) => "Serial: 16950 UART",
        (0x07, 0x80) => "Serial: Other",
        (0x08, 0x00) => "Interrupt: 8259 PIC",
        (0x08, 0x01) => "Interrupt: APIC",
        (0x08, 0x02) => "Interrupt: IOAPIC",
        (0x08, 0x03) => "Interrupt: MSI",
        (0x08, 0x04) => "Interrupt: HT-APIC",
        (0x08, 0x80) => "Interrupt: Other",
        (0x09, 0x00) => "Input: Keyboard",
        (0x09, 0x01) => "Input: Pen",
        (0x09, 0x02) => "Input: Mouse",
        (0x09, 0x03) => "Input: Scanner",
        (0x09, 0x04) => "Input: Gameport",
        (0x0a, 0x00) => "Docking: Generic",
        (0x0b, 0x00) => "Processor: 386",
        (0x0b, 0x01) => "Processor: 486",
        (0x0b, 0x02) => "Processor: Pentium",
        (0x0b, 0x10) => "Processor: Alpha",
        (0x0c, 0x00) => "Serial Bus: FireWire",
        (0x0c, 0x01) => "Serial Bus: ACCESS.bus",
        (0x0c, 0x02) => "Serial Bus: SSA",
        (0x0c, 0x03) => "Serial Bus: USB (UHCI/EHCI)",
        (0x0c, 0x04) => "Serial Bus: Fibre Channel",
        (0x0c, 0x05) => "Serial Bus: SMBus",
        (0x0c, 0x06) => "Serial Bus: InfiniBand",
        (0x0c, 0x07) => "Serial Bus: IPMI",
        (0x0c, 0x08) => "Serial Bus: SERCOS",
        (0x0c, 0x09) => "Serial Bus: CANbus",
        (0x0d, 0x00) => "Wireless: IRDA",
        (0x0d, 0x01) => "Wireless: IR",
        (0x0d, 0x02) => "Wireless: RF",
        (0x0d, 0x03) => "Wireless: Bluetooth",
        (0x0d, 0x04) => "Wireless: Broadband",
        (0x0e, 0x00) => "I2C: I2C",
        (0x0e, 0x01) => "I2C: SMBus",
        (0x0f, 0x00) => "Satellite: TV",
        (0x0f, 0x01) => "Satellite: Audio",
        (0x0f, 0x02) => "Satellite: Voice",
        (0x0f, 0x03) => "Satellite: Data",
        (0x10, 0x00) => "Encryption: Network",
        (0x10, 0x01) => "Encryption: Entertainment",
        (0x10, 0x80) => "Encryption: Other",
        (0x11, 0x00) => "DSP: DPIO",
        (0x11, 0x01) => "DSP: Other",
        _ => "Other",
    }
}

fn eprint(s: &str) { let _ = write(2, s.as_bytes()); }

fn scan_pci() {
    let dir_fd = open(PCI_DEVS.as_ptr(), 0);
    if dir_fd >= 0xFFFF_FFFF_FFFF_FF00 {
        eprint("[vahid] sysfs not found, using legacy scan\n");
        scan_legacy_pci();
        return;
    }
    eprint("[vahid] PCI devices:\n");
    let mut buf = [0u8; 4096];
    let n = getdents64(dir_fd, buf.as_mut_ptr(), buf.len());
    if n > 0 {
        let mut off = 0;
        let n = n as usize;
        while off + 18 < n {
            let reclen = u16::from_ne_bytes([buf[off+16], buf[off+17]]) as usize;
            if reclen < 19 || off + reclen > n { break; }
            let name_end = off + 18 + buf[off+18..off+reclen].iter().position(|&b| b == 0).unwrap_or(reclen - 19);
            let name = core::str::from_utf8(&buf[off+18..name_end]).unwrap_or("");
            if name != "." && name != ".." {
                let parts: Vec<&str> = name.split(':').collect();
                let (vid_str, did_str) = if parts.len() >= 2 {
                    let slot = parts[parts.len()-2];
                    let func = parts[parts.len()-1];
                    if slot.len() >= 4 && func.len() >= 4 {
                        (u16::from_str_radix(&slot[..4], 16).unwrap_or(0),
                         u16::from_str_radix(&func[..4], 16).unwrap_or(0))
                    } else { (0, 0) }
                } else { (0, 0) };
                if vid_str != 0 {
                    let msg = alloc::format!("  {:04x}:{:04x} {} - {} ({})\n",
                        vid_str, did_str, vendor_name(vid_str), device_name(vid_str, did_str), name);
                    eprint(&msg);
                } else {
                    let msg = alloc::format!("  {} (no vendor info)\n", name);
                    eprint(&msg);
                }
            }
            off += reclen;
        }
    }
    close(dir_fd);
}

fn scan_legacy_pci() {
    eprint("[vahid] legacy PCI scan (I/O port cfg):\n");
    for bus in 0..=0 {
        for slot in 0..32 {
            for func in 0..8 {
                let addr = 0x8000_0000u32 | (bus << 16) | (slot << 11) | (func << 8);
                let vid = unsafe { pci_read_config(addr, 0) as u16 };
                if vid != 0 && vid != 0xFFFF {
                    let did = unsafe { pci_read_config(addr, 2) as u16 };
                    let cc = unsafe { (pci_read_config(addr, 10) >> 16) as u8 };
                    let sc = unsafe { ((pci_read_config(addr, 10) >> 8) & 0xFF) as u8 };
                    let msg = alloc::format!("  {:02x}:{:02x}.{:02x}  {:04x}:{:04x}  {}  {}\n",
                        bus, slot, func, vid, did, class_name(cc, sc), vendor_name(vid));
                    eprint(&msg);
                    if vid == 0xFFFF { break; }
                } else {
                    if func == 0 { break; }
                }
            }
        }
    }
}

unsafe fn pci_read_config(addr: u32, offset: u8) -> u32 {
    let config_addr = 0x8000_0000u64 | (addr as u64) | (offset as u64 & 0xFC);
    skyos_libc::syscall::syscall1(0x1000, config_addr) as u32
}

fn create_devices() {
    let dev_path = CString::new("/dev/").unwrap();
    let dev_fd = open(dev_path.as_ptr() as *const u8, 0);
    if dev_fd >= 0xFFFF_FFFF_FFFF_FF00 {
        eprint("[vahid] /dev not available\n");
        return;
    }
    close(dev_fd);

    for (_, _, devices) in VENDORS {
        for (did, _) in *devices {
            if *did == 0x1001 {
                let path = alloc::format!("/dev/vda");
                let c = CString::new(path.as_bytes()).unwrap();
                let ret = skyos_libc::syscall::syscall2(0x1001, c.as_ptr() as u64, 0x1001);
                if ret >= 0xFFFF_FFFF_FFFF_FF00 {
                    eprint("[vahid] /dev/vda: no virtio block device\n");
                }
            }
        }
    }

    let nodes = &["null", "zero", "random", "urandom", "tty", "console"];
    for name in nodes {
        let path = alloc::format!("/dev/{}", name);
        let c = CString::new(path.as_bytes()).unwrap();
        let fd = open(c.as_ptr() as *const u8, 0x41);
        if fd < 0xFFFF_FFFF_FFFF_FF00 { close(fd); }
    }
    eprint("[vahid] /dev/null, zero, random, urandom, tty, console ready\n");
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    eprint("[vahid] SkyOS Device Manager v0.2\n");
    scan_pci();
    create_devices();
    eprint("[vahid] ready\n");
    loop { unsafe { core::arch::asm!("pause"); } }
}
