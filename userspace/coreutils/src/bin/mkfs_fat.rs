#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

const BLKGETSIZE64: u64 = 0x80081272;
const BLKWR_SEC: u64 = 0x40001261;

#[repr(C, packed)]
struct BlkIoctlOp {
    sector: u64,
    count: u64,
    buf: u64,
}

fn write_sectors(fd: u64, sector: u64, count: u64, buf: &[u8]) -> bool {
    let op = BlkIoctlOp { sector, count, buf: buf.as_ptr() as u64 };
    skyos_libc::syscall::ioctl(fd, BLKWR_SEC, &op as *const _ as *mut u8) == 0
}

fn format_fat32(fd: u64, total_sectors: u64, label: &str) -> bool {
    let bytes_per_sector: u16 = 512;
    let sectors_per_cluster: u8 = 8;
    let reserved_sectors: u16 = 32;
    let fat_count: u8 = 2;
    let root_entries: u16 = 0;
    let total_sectors_16: u16 = 0;
    let media: u8 = 0xF8;
    let sectors_per_fat: u32 = ((total_sectors as u32 - reserved_sectors as u32) / (sectors_per_cluster as u32 * 128 + 2) + 1).max(32);
    let sectors_per_track: u16 = 32;
    let heads: u16 = 64;
    let hidden_sectors: u32 = 0;
    let total_sectors_32: u32 = total_sectors as u32;
    let fs_version: u16 = 0;
    let root_cluster: u32 = 2;
    let fs_info_sector: u16 = 1;
    let backup_boot_sector: u16 = 6;

    // Boot sector
    let mut boot = vec![0u8; 512];
    boot[0] = 0xEB; boot[1] = 0x58; boot[2] = 0x90; // jmp + nop
    boot[3..11].copy_from_slice(b"SKYOSFAT");
    boot[11..13].copy_from_slice(&bytes_per_sector.to_le_bytes());
    boot[13] = sectors_per_cluster;
    boot[14..16].copy_from_slice(&reserved_sectors.to_le_bytes());
    boot[16] = fat_count;
    boot[17..19].copy_from_slice(&root_entries.to_le_bytes());
    boot[19..21].copy_from_slice(&total_sectors_16.to_le_bytes());
    boot[21] = media;
    boot[22..24].copy_from_slice(&sectors_per_fat.to_le_bytes() as &[u8]);
    boot[24..26].copy_from_slice(&sectors_per_track.to_le_bytes());
    boot[26..28].copy_from_slice(&heads.to_le_bytes());
    boot[28..32].copy_from_slice(&hidden_sectors.to_le_bytes());
    boot[32..36].copy_from_slice(&total_sectors_32.to_le_bytes());
    boot[36] = sectors_per_fat as u8; boot[37] = (sectors_per_fat >> 8) as u8;
    boot[38..40].copy_from_slice(&(0 as u16).to_le_bytes()); // flags
    boot[40..42].copy_from_slice(&fs_version.to_le_bytes());
    boot[42..44].copy_from_slice(&root_cluster.to_le_bytes() as &[u8]);
    boot[44..46].copy_from_slice(&fs_info_sector.to_le_bytes());
    boot[46..48].copy_from_slice(&backup_boot_sector.to_le_bytes());
    boot[48..64].fill(0); // reserved
    boot[64..66].copy_from_slice(&0u16.to_le_bytes()); // drive number
    boot[66] = 0; // reserved1
    boot[67] = 0x29; // extended boot sig
    boot[68..72].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]); // volume serial
    let label_bytes = label.as_bytes();
    let max_label = core::cmp::min(label_bytes.len(), 11);
    boot[71..71+max_label].copy_from_slice(&label_bytes[..max_label]);
    boot[82..90].copy_from_slice(b"FAT32   ");
    boot[510] = 0x55; boot[511] = 0xAA;

    if !write_sectors(fd, 0, 1, &boot) { return false; }

    // FSInfo sector (sector 1)
    let mut fsinfo = vec![0u8; 512];
    fsinfo[0..4].copy_from_slice(b"RRaA");
    fsinfo[484..488].copy_from_slice(b"rrAa");
    fsinfo[488..492].copy_from_slice(&2u32.to_le_bytes()); // free clusters hint
    fsinfo[492..496].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes()); // next free cluster
    fsinfo[508..512].copy_from_slice(&[0x00, 0x00, 0x55, 0xAA]);
    if !write_sectors(fd, 1, 1, &fsinfo) { return false; }

    // Backup boot sectors
    if !write_sectors(fd, backup_boot_sector as u64, 1, &boot) { return false; }
    if !write_sectors(fd, backup_boot_sector as u64 + 1, 1, &fsinfo) { return false; }

    // FAT tables
    let fat_sectors = sectors_per_fat as u64;
    let mut fat = vec![0u8; 512 * fat_sectors as usize];
    // Cluster 0: media descriptor
    fat[0] = media; fat[1] = 0xFF; fat[2] = 0xFF; fat[3] = 0xFF;
    // Cluster 1: end marker
    fat[4] = 0xFF; fat[5] = 0xFF; fat[6] = 0xFF; fat[7] = 0xFF;
    // Cluster 2: root directory (end of chain)
    fat[8] = 0xFF; fat[9] = 0xFF; fat[10] = 0xFF; fat[11] = 0xFF;

    for f in 0..fat_count {
        let fat_start = reserved_sectors as u64 + f as u64 * fat_sectors;
        if !write_sectors(fd, fat_start, fat_sectors, &fat) { return false; }
    }

    // Root directory cluster
    let data_start = reserved_sectors as u64 + fat_count as u64 * fat_sectors;
    let root_dir = vec![0u8; 512 * sectors_per_cluster as usize];
    if !write_sectors(fd, data_start, sectors_per_cluster as u64, &root_dir) { return false; }

    true
}

#[no_mangle]
pub extern "C" fn main(argc: u64, argv: *const *const u8) -> i32 {
    if argc < 2 {
        skyos_libc::syscall::write(2, b"Usage: mkfs.fat [-n label] <device>\n");
        return 1;
    }

    let mut device: Option<&str> = None;
    let mut label = "SKYOS FAT";

    let mut i = 1;
    while (i as u64) < argc {
        let arg = unsafe {
            let ptr = *argv.add(i);
            if ptr.is_null() { break; }
            core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
        };
        match arg {
            "-n" => {
                i += 1;
                label = unsafe {
                    let ptr = *argv.add(i);
                    core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("SKYOS FAT")
                };
            }
            _ => device = Some(arg),
        }
        i += 1;
    }

    let dev = match device { Some(d) => d, None => { return 1; } };
    let cpath = alloc::ffi::CString::new(dev).unwrap();
    let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 1);
    if (fd as i64) < 0 {
        let msg = alloc::format!("mkfs.fat: cannot open {}\n", dev);
        skyos_libc::syscall::write(2, msg.as_bytes());
        return 1;
    }

    let mut size: u64 = 0;
    skyos_libc::syscall::ioctl(fd, BLKGETSIZE64, &mut size as *mut u64 as *mut u8);
    if size < 1024 * 1024 {
        skyos_libc::syscall::write(2, b"mkfs.fat: device too small (min 1 MB)\n");
        skyos_libc::syscall::close(fd);
        return 1;
    }

    let sectors = size / 512;
    let msg = alloc::format!("mkfs.fat: Creating FAT32 on {} ({} sectors)...\n", dev, sectors);
    skyos_libc::syscall::write(1, msg.as_bytes());

    if format_fat32(fd, sectors, label) {
        skyos_libc::syscall::write(1, b"mkfs.fat: Done.\n");
        skyos_libc::syscall::close(fd);
        0
    } else {
        skyos_libc::syscall::write(2, b"mkfs.fat: Failed to create filesystem\n");
        skyos_libc::syscall::close(fd);
        1
    }
}
