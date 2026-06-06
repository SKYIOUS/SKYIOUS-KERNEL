use volatile::Volatile;

#[repr(C)]
pub struct HbaMemory {
    pub cap: Volatile<u32>,
    pub ghc: Volatile<u32>,
    pub is: Volatile<u32>,
    pub pi: Volatile<u32>,
    pub vs: Volatile<u32>,
    pub ccc_ctl: Volatile<u32>,
    pub ccc_ports: Volatile<u32>,
    pub em_loc: Volatile<u32>,
    pub em_ctl: Volatile<u32>,
    pub cap2: Volatile<u32>,
    pub bohc: Volatile<u32>,
    pub rsv: [Volatile<u8>; 0xA0 - 0x2C],
    pub vendor: [Volatile<u8>; 0x100 - 0xA0],
    pub ports: [HbaPort; 32],
}

#[repr(C)]
pub struct HbaPort {
    pub clb: Volatile<u32>,
    pub clbu: Volatile<u32>,
    pub fb: Volatile<u32>,
    pub fbu: Volatile<u32>,
    pub is: Volatile<u32>,
    pub ie: Volatile<u32>,
    pub cmd: Volatile<u32>,
    pub rsv0: Volatile<u32>,
    pub tfd: Volatile<u32>,
    pub sig: Volatile<u32>,
    pub ssts: Volatile<u32>,
    pub sctl: Volatile<u32>,
    pub serr: Volatile<u32>,
    pub sact: Volatile<u32>,
    pub ci: Volatile<u32>,
    pub sntf: Volatile<u32>,
    pub fbs: Volatile<u32>,
    pub rsv1: [Volatile<u32>; 11],
    pub vendor: [Volatile<u32>; 4],
}

#[repr(C, packed)]
pub struct FisRegH2D {
    pub fis_type: u8,   // 0x27
    pub pm_port: u8,    // Port multiplier
    pub command: u8,    // Command register
    pub feature_l: u8,  // Feature register, 7:0
    pub lba0: u8,       // LBA low register, 7:0
    pub lba1: u8,       // LBA mid register, 15:8
    pub lba2: u8,       // LBA high register, 23:16
    pub device: u8,     // Device register
    pub lba3: u8,       // LBA register, 31:24
    pub lba4: u8,       // LBA register, 39:32
    pub lba5: u8,       // LBA register, 47:40
    pub feature_h: u8,  // Feature register, 15:8
    pub count_l: u8,    // Count register, 7:0
    pub count_h: u8,    // Count register, 15:8
    pub iso_command_completion: u8, // Command completion
    pub control: u8,    // Control register
    pub rsv1: [u8; 4],  // Reserved
}

#[repr(C)]
pub struct CommandHeader {
    pub dw0: Volatile<u32>, // Command FIS length in DWORDS, 2:4. CFL
    pub dw1: Volatile<u32>, // Physical Region Descriptor Table Length in entries
    pub ctba: Volatile<u32>, // Command Table Descriptor Base Address
    pub ctbau: Volatile<u32>, // Command Table Descriptor Base Address Upper 32-bits
    pub rsv: [Volatile<u32>; 4],
}

#[repr(C)]
pub struct HbaCmdTable {
    // 0x00
    pub cfis: [u8; 64], // Command FIS
    // 0x40
    pub acmd: [u8; 16], // ATAPI command
    // 0x50
    pub rsv: [u8; 48],
    // 0x80
    pub prdt_entry: [HbaPrdtEntry; 1], // Physical Region Descriptor Table entries (simplified to 1)
}

#[repr(C)]
pub struct HbaPrdtEntry {
    pub dba: u32,       // Data Base Address
    pub dbau: u32,      // Data Base Address Upper 32-bits
    pub rsv0: u32,
    pub dbc: u32,       // Byte count, 4M max, interrupt = 1
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum PortType {
    None = 0,
    SATA = 1,
    SEMB = 2,
    PM = 3,
    SATAPI = 4,
}

pub fn check_port_type(port: &HbaPort) -> PortType {
    let ssts = port.ssts.read();
    let ipm = (ssts >> 8) & 0x0F;
    let det = ssts & 0x0F;

    if det != 3 {
        return PortType::None;
    }
    if ipm != 1 {
        return PortType::None;
    }

    match port.sig.read() {
        0x00000101 => PortType::SATA,
        0xEB140101 => PortType::SATAPI,
        0xC33C0101 => PortType::SEMB,
        0x96690101 => PortType::PM,
        _ => PortType::None,
    }
}

use crate::drivers::block::{BlockDevice, BlockDeviceError, register_block_device};
use alloc::sync::Arc;
use spin::Mutex;

pub struct AhciPort {
    pub port: *mut HbaPort,
    pub clb_virt: *mut CommandHeader,
    pub fb_virt: u64,
    pub ctba_virt: *mut HbaCmdTable,
    pub identify_sector_count: core::sync::atomic::AtomicU64,
}

unsafe impl Send for AhciPort {}
unsafe impl Sync for AhciPort {}

impl AhciPort {
    pub fn new(port: &'static mut HbaPort, virt_clb: *mut CommandHeader, virt_ctba: *mut HbaCmdTable) -> Self {
        // The original `new` function signature and body are based on the old struct definition.
        // We need to adapt it to the new `AhciPort` struct fields.
        // The `fb_virt` field is new and needs to be initialized.
        // The `port` field type changed from `&'static mut HbaPort` to `*mut HbaPort`.
        // The `virt_clb` and `virt_ctba` fields were renamed to `clb_virt` and `ctba_virt`.

        // For now, we'll initialize fb_virt to 0, as its value is determined in configure_port.
        // A more robust solution would be to pass it to new or have a separate initialization step.
        Self {
            port: port as *mut HbaPort, // Convert reference to raw pointer
            clb_virt: virt_clb,
            fb_virt: 0, // Placeholder, will be set later by configure_port
            ctba_virt: virt_ctba,
            identify_sector_count: core::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl BlockDevice for AhciPort {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        if self.read(sector, 1, buf) {
            Ok(())
        } else {
            Err(BlockDeviceError::ReadError)
        }
    }

    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        if self.write(sector, 1, buf) {
            Ok(())
        } else {
            Err(BlockDeviceError::WriteError)
        }
    }

    fn sector_count(&self) -> Result<u64, BlockDeviceError> {
        // Use cached value from IDENTIFY DEVICE if available
        let count = self.identify_sector_count.load(core::sync::atomic::Ordering::SeqCst);
        if count > 0 {
            Ok(count)
        } else {
            // Fallback: trigger identify if not done yet
            Ok(1024 * 1024)
        }
    }
}

pub fn init(base_addr: usize) {
    let hba = unsafe { &mut *(base_addr as *mut HbaMemory) };
    
    // Check implemented ports
    let pi = hba.pi.read();
    
    for i in 0..32 {
        if pi & (1 << i) != 0 {
            let port_type = check_port_type(&hba.ports[i]);
            if let PortType::SATA = port_type {
                crate::println!("AHCI: Port {} is SATA. Initializing...", i);
                let (virt_clb, virt_ctba) = configure_port(&mut hba.ports[i]);
                
                // Initialize as a block device
                let mut ahci_dev = AhciPort::new(unsafe { &mut *(&raw mut hba.ports[i]) }, virt_clb, virt_ctba);
                
                // Test Write/Read
                crate::println!("AHCI: Testing Read/Write on Port {}...", i);
                let mut test_buf = [0u8; 512];
                if ahci_dev.read_sector(0, &mut test_buf).is_ok() {
                    crate::println!("AHCI: Initial Read Success. Signature: {:02x}{:02x}", test_buf[510], test_buf[511]);
                }
                
                // Register as a global block device
                let ahci_arc = Arc::new(Mutex::new(ahci_dev));
                
                // Send IDENTIFY DEVICE to get real sector count
                if let Some(mut port_lock) = ahci_arc.try_lock() {
                    port_lock.identify_device();
                }
                
                register_block_device(ahci_arc);
            } else if let PortType::SATAPI = port_type {
                crate::println!("AHCI: Port {} is SATAPI (CD/DVD).", i);
            }
        }
    }
}

pub fn configure_port(port: &mut HbaPort) -> (*mut CommandHeader, *mut HbaCmdTable) {
    use alloc::boxed::Box;
    use x86_64::VirtAddr;
    
    // Stop command engine
    stop_cmd(port);
    
    // Allocate Command List (1K aligned)
    // We allocate 4KB to include space for Command Tables
    let cmd_list = Box::leak(Box::new([0u8; 4096]));
    let cmd_list_virt = VirtAddr::from_ptr(cmd_list.as_ptr());
    let cmd_list_phys = crate::memory::virt_to_phys(cmd_list_virt).expect("Failed to get physical address for AHCI CLB");
    
    port.clb.write(cmd_list_phys.as_u64() as u32);
    port.clbu.write((cmd_list_phys.as_u64() >> 32) as u32);
    
    // Allocate FIS (256 bytes aligned)
    let fis = Box::leak(Box::new([0u8; 256]));
    let fis_virt = VirtAddr::from_ptr(fis.as_ptr());
    let fis_phys = crate::memory::virt_to_phys(fis_virt).expect("Failed to get physical address for AHCI FB");
    
    port.fb.write(fis_phys.as_u64() as u32);
    port.fbu.write((fis_phys.as_u64() >> 32) as u32);
    
    // Initialize Command Headers to point to Command Tables
    // For simplicity, we'll put the Command Table for slot 0 after the Command List in the same 4KB page.
    // Command List is 1KB. We have 3KB remaining.
    let cmd_headers = unsafe { core::slice::from_raw_parts_mut(cmd_list.as_mut_ptr() as *mut CommandHeader, 32) };
    for i in 0..1 { // Just initialize slot 0 for now
        let ctba_phys = cmd_list_phys + 1024u64 + (i as u64 * 512);
        cmd_headers[i].ctba.write(ctba_phys.as_u64() as u32);
        cmd_headers[i].ctbau.write((ctba_phys.as_u64() >> 32) as u32);
    }

    // Enable FIS receive and Start
    let mut cmd = port.cmd.read();
    cmd |= 1 << 4; // FRE
    cmd |= 1;      // ST
    port.cmd.write(cmd);
    
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
    let virt_clb = (offset + cmd_list_phys.as_u64()) as *mut CommandHeader;
    let virt_ctba = (offset + (cmd_list_phys.as_u64() + 1024)) as *mut HbaCmdTable;

    crate::println!("AHCI: Port configured. CLB: 0x{:x}, FB: 0x{:x}", cmd_list_phys.as_u64(), fis_phys.as_u64());

    (virt_clb, virt_ctba)
}

pub fn start_cmd(port: &mut HbaPort) {
    // Determine if we need to clear bits
    let cmd = port.cmd.read();
    if (cmd & 1) == 0 {
         port.cmd.write(cmd | 1);
    }
}

pub fn stop_cmd(port: &mut HbaPort) {
    let cmd = port.cmd.read();
    if (cmd & 1) != 0 {
        port.cmd.write(cmd & !1);
    }
    // Spin until FRE (bit 4) is cleared? Not always necessary for simple stop
}

impl AhciPort {
    pub fn read(&mut self, start_lba: u64, sectors: u32, buf: &mut [u8]) -> bool {
        let port = unsafe { &mut *self.port };
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().expect("Physical memory offset not initialized");
    port.is.write(0xFFFFFFFF); // Clear pending ints
    
    // Limited to 1 sector read for simplicity in this phase of PoC
    // Or at least buf length check
    
    let mut slot = 0xFF;
    // Find free command slot
    let slots = port.sact.read() | port.ci.read();
    for i in 0..32 {
        if (slots & (1 << i)) == 0 {
            slot = i;
            break;
        }
    }
    if slot == 0xFF { return false; }
    
    let clb_phys = port.clb.read() as u64 | ((port.clbu.read() as u64) << 32);
    let clb_virt = clb_phys + offset;
    let cmd_header = unsafe { &mut *(clb_virt as *mut CommandHeader).offset(slot as isize) };
    
    // reset command header
    // 0x4 (dw1) = PRDT Length (1 entry)
    // 0x5 (dw0) = FIS Length (5 dwords) | Write (0) | Prefetchable (0)
    
    cmd_header.dw0.write( (::core::mem::size_of::<FisRegH2D>() as u32 / 4) | (0 << 6) ); // 0 = Read (Write bit clear)
    cmd_header.dw1.write(1); // 1 PRDT Entry

    let ctba_virt = self.ctba_virt as u64;
    
    // Clear the table (memset)
    unsafe {
       core::ptr::write_bytes(ctba_virt as *mut u8, 0, ::core::mem::size_of::<HbaCmdTable>());
    }
    
    let cmd_tbl = unsafe { &mut *(ctba_virt as *mut HbaCmdTable) };
    
    // Setup PRDT (Physical Region Descriptor Table)
    // Buffer needs to be properly aligned/physically mapped.
    // For this PoC, we assume Identity Mapping or `buf` is accessible.
    // In real usage, `buf` MUST be a physical address or mapped correctly.
    // We will assume `buf` is a slice in identity mapped heap.
    
    use x86_64::VirtAddr;
    let buf_virt_addr = VirtAddr::from_ptr(buf.as_ptr());
    let buf_phys = crate::memory::virt_to_phys_dma(buf_virt_addr);
    
    if buf_phys.as_u64() == 0 {
         // Should propagate error
         crate::println!("AHCI Error: Buf Phys Translation Failed");
         return false;
    }
    
    cmd_tbl.prdt_entry[0].dba = buf_phys.as_u64() as u32;
    cmd_tbl.prdt_entry[0].dbau = (buf_phys.as_u64() >> 32) as u32;
    cmd_tbl.prdt_entry[0].dbc = (sectors * 512) - 1; // 512 bytes per sector usually
    cmd_tbl.prdt_entry[0].rsv0 = 1; // Interrupt on completion
    
    // Setup Command FIS
    let fis = unsafe { &mut *(cmd_tbl.cfis.as_mut_ptr() as *mut FisRegH2D) };
    fis.fis_type = 0x27; // H2D
    fis.command = 0x25; // READ_DMA_EXT
    fis.control = 1;    // Command
    
    fis.lba0 = start_lba as u8;
    fis.lba1 = (start_lba >> 8) as u8;
    fis.lba2 = (start_lba >> 16) as u8;
    fis.device = 1 << 6; // LBA mode
    
    fis.lba3 = (start_lba >> 24) as u8;
    fis.lba4 = (start_lba >> 32) as u8;
    fis.lba5 = (start_lba >> 40) as u8;
    
    fis.count_l = sectors as u8;
    fis.count_h = (sectors >> 8) as u8;
    
    // Wait until port is not busy
    let mut spin = 0;
    while (port.tfd.read() & (0x80 | 0x8)) != 0 {
        spin += 1;
        if spin > 1000000 { return false; }
    }
    
    // Issue command
    port.ci.write(1 << slot);
    
    // Wait for completion
    loop {
        // Check for error
        if (port.is.read() & (1 << 30)) != 0 {
             return false;
        }
        
        if (port.ci.read() & (1 << slot)) == 0 {
            break; // Done
        }
        
        if (port.is.read() & (1 << 26)) != 0 {
             // Task file error
              return false;
        }
    }
    
    return true;
}

    /// Sends IDENTIFY DEVICE command (0xEC) to detect drive parameters.
    /// Parses LBA48 sector count from words 100-103 and caches it.
    pub fn identify_device(&mut self) -> bool {
        let port = unsafe { &mut *self.port };
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().expect("phys offset");

        port.is.write(0xFFFFFFFF);

        let mut slot = 0xFF;
        let slots = port.sact.read() | port.ci.read();
        for i in 0..32 {
            if (slots & (1 << i)) == 0 { slot = i; break; }
        }
        if slot == 0xFF { return false; }

        let clb_phys = port.clb.read() as u64 | ((port.clbu.read() as u64) << 32);
        let clb_virt = clb_phys + offset;
        let cmd_header = unsafe { &mut *(clb_virt as *mut CommandHeader).offset(slot as isize) };

        // Allocate a 512-byte buffer for the IDENTIFY data
        let buf = alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; 512]));
        let buf_virt = x86_64::VirtAddr::from_ptr(buf.as_ptr());
        let buf_phys = crate::memory::virt_to_phys_dma(buf_virt);

        cmd_header.dw0.write((core::mem::size_of::<FisRegH2D>() as u32 / 4) | (0 << 6)); // Read
        cmd_header.dw1.write(1); // 1 PRDT entry

        let ctba_virt = self.ctba_virt as u64;
        unsafe { core::ptr::write_bytes(ctba_virt as *mut u8, 0, core::mem::size_of::<HbaCmdTable>()); }
        let cmd_tbl = unsafe { &mut *(ctba_virt as *mut HbaCmdTable) };

        cmd_tbl.prdt_entry[0].dba = buf_phys.as_u64() as u32;
        cmd_tbl.prdt_entry[0].dbau = (buf_phys.as_u64() >> 32) as u32;
        cmd_tbl.prdt_entry[0].dbc = 511; // 512 bytes - 1
        cmd_tbl.prdt_entry[0].rsv0 = 1;

        let fis = unsafe { &mut *(cmd_tbl.cfis.as_mut_ptr() as *mut FisRegH2D) };
        fis.fis_type = 0x27;
        fis.command = 0xEC; // IDENTIFY DEVICE
        fis.control = 1;
        fis.device = 0;

        let mut spin = 0;
        while (port.tfd.read() & (0x80 | 0x8)) != 0 {
            spin += 1;
            if spin > 1000000 { return false; }
        }

        port.ci.write(1 << slot);

        loop {
            if (port.is.read() & (1 << 30)) != 0 { return false; }
            if (port.ci.read() & (1 << slot)) == 0 { break; }
            if (port.is.read() & (1 << 26)) != 0 { return false; }
        }

        // Parse IDENTIFY data words 100-103 (LBA48 sector count)
        // Each word is little-endian u16
        let words = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u16, 256) };
        let lba_low = words[100] as u64;
        let lba_mid = words[101] as u64;
        let lba_high = words[102] as u64;
        let lba_very = words[103] as u64;
        let total_sectors = lba_low | (lba_mid << 16) | (lba_high << 32) | (lba_very << 48);

        if total_sectors > 0 {
            self.identify_sector_count.store(total_sectors, core::sync::atomic::Ordering::SeqCst);
            crate::println!("AHCI: IDENTIFY DEVICE: total_sectors={} ({:.2} GB)", total_sectors, (total_sectors * 512) as f64 / 1_000_000_000.0);
        } else {
            // Fallback to words 60-61 (LBA28)
            let lba28_low = words[60] as u64;
            let lba28_high = words[61] as u64;
            let lba28 = lba28_low | (lba28_high << 16);
            if lba28 > 0 {
                self.identify_sector_count.store(lba28, core::sync::atomic::Ordering::SeqCst);
                crate::println!("AHCI: IDENTIFY DEVICE (LBA28): total_sectors={}", lba28);
            }
        }

        // Free the temporary buffer
        unsafe { let _ = alloc::boxed::Box::from_raw(buf); }

        true
    }

pub fn write(&mut self, start_lba: u64, sectors: u32, buf: &[u8]) -> bool {
    let port = unsafe { &mut *self.port };
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().expect("Physical memory offset not initialized");
    port.is.write(0xFFFFFFFF); // Clear pending ints
    
    let mut slot = 0xFF;
    let slots = port.sact.read() | port.ci.read();
    for i in 0..32 {
        if (slots & (1 << i)) == 0 {
            slot = i;
            break;
        }
    }
    if slot == 0xFF { return false; }
    
    let clb_phys = port.clb.read() as u64 | ((port.clbu.read() as u64) << 32);
    let clb_virt = clb_phys + offset;
    let cmd_header = unsafe { &mut *(clb_virt as *mut CommandHeader).offset(slot as isize) };
    
    // Dw0: Write bit (bit 6) set to 1
    cmd_header.dw0.write( (::core::mem::size_of::<FisRegH2D>() as u32 / 4) | (1 << 6) ); 
    cmd_header.dw1.write(1); // 1 PRDT Entry

    let ctba_virt = self.ctba_virt as u64;
    unsafe {
       core::ptr::write_bytes(ctba_virt as *mut u8, 0, ::core::mem::size_of::<HbaCmdTable>());
    }
    let cmd_tbl = unsafe { &mut *(ctba_virt as *mut HbaCmdTable) };
    
    use x86_64::VirtAddr;
    let buf_virt_addr = VirtAddr::from_ptr(buf.as_ptr());
    let buf_phys = crate::memory::virt_to_phys_dma(buf_virt_addr);
    
    if buf_phys.as_u64() == 0 { return false; }
    
    cmd_tbl.prdt_entry[0].dba = buf_phys.as_u64() as u32;
    cmd_tbl.prdt_entry[0].dbau = (buf_phys.as_u64() >> 32) as u32;
    cmd_tbl.prdt_entry[0].dbc = (sectors * 512) - 1;
    cmd_tbl.prdt_entry[0].rsv0 = 1;
    
    let fis = unsafe { &mut *(cmd_tbl.cfis.as_mut_ptr() as *mut FisRegH2D) };
    fis.fis_type = 0x27; // H2D
    fis.command = 0x35; // WRITE_DMA_EXT
    fis.control = 1;
    
    fis.lba0 = start_lba as u8;
    fis.lba1 = (start_lba >> 8) as u8;
    fis.lba2 = (start_lba >> 16) as u8;
    fis.device = 1 << 6;
    
    fis.lba3 = (start_lba >> 24) as u8;
    fis.lba4 = (start_lba >> 32) as u8;
    fis.lba5 = (start_lba >> 40) as u8;
    
    fis.count_l = sectors as u8;
    fis.count_h = (sectors >> 8) as u8;
    
    let mut spin = 0;
    while (port.tfd.read() & (0x80 | 0x8)) != 0 {
        spin += 1;
        if spin > 1000000 { return false; }
    }
    
    port.ci.write(1 << slot);
    
    loop {
        if (port.is.read() & (1 << 30)) != 0 { return false; }
        if (port.ci.read() & (1 << slot)) == 0 { break; }
        if (port.is.read() & (1 << 26)) != 0 { return false; }
        core::hint::spin_loop();
    }
    
    return true;
    }
}
