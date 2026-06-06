#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;


#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

const BLKGETSIZE64: u64 = 0x80081272;
const BLKRD_SEC: u64 = 0x40001260;

#[repr(C, packed)]
struct BlkIoctlOp {
    sector: u64,
    count: u64,
    buf: u64,
}

#[repr(C, packed)]
struct MbrPartEntry {
    boot_flag: u8,
    chs_start: [u8; 3],
    type_: u8,
    chs_end: [u8; 3],
    lba_start: u32,
    sector_count: u32,
}

fn read_sectors(fd: u64, sector: u64, count: u64, buf: &mut [u8]) -> bool {
    let op = BlkIoctlOp { sector, count, buf: buf.as_ptr() as u64 };
    skyos_libc::syscall::ioctl(fd, BLKRD_SEC, &op as *const _ as *mut u8) == 0
}

fn part_type_name(t: u8) -> &'static str {
    match t {
        0x01 => "FAT12", 0x04 => "FAT16", 0x05 => "Extended",
        0x06 => "FAT16B", 0x07 => "NTFS/HPFS", 0x0B => "FAT32",
        0x0C => "FAT32 LBA", 0x0E => "FAT16 LBA", 0x0F => "Extended LBA",
        0x11 => "Hidden FAT12", 0x14 => "Hidden FAT16",
        0x17 => "Hidden NTFS", 0x1B => "Hidden FAT32",
        0x1C => "Hidden FAT32 LBA", 0x1E => "Hidden FAT16 LBA",
        0x27 => "WinRE", 0x3C => "PMagic", 0x42 => "PReP",
        0x82 => "Linux swap", 0x83 => "Linux", 0x84 => "Hibernation",
        0x85 => "Linux extended", 0x86 => "NTFS volset", 0x87 => "NTFS volset",
        0xA0 => "Hibernation", 0xA8 => "Mac X", 0xAB => "Mac boot",
        0xAF => "Mac HFS+", 0xB7 => "BSDI", 0xB8 => "BSDI swap",
        0xEE => "GPT protective", 0xEF => "EFI sys", 0xFB => "VMware FS",
        0xFC => "VMware swap", _ => "Unknown",
    }
}

fn parse_mbr(buf: &[u8; 512]) -> Vec<(u8, u32, u32, &'static str)> {
    let mut parts = Vec::new();
    if buf[510] != 0x55 || buf[511] != 0xAA { return parts; }
    for i in 0..4 {
        let off = 0x1BE + i * 16;
        let entry: MbrPartEntry = unsafe { core::ptr::read_unaligned(buf.as_ptr().add(off) as *const MbrPartEntry) };
        if entry.type_ == 0 || entry.sector_count == 0 { continue; }
        parts.push(((i + 1) as u8, entry.lba_start, entry.sector_count, part_type_name(entry.type_)));
    }
    parts
}

fn parse_gpt(buf: &[u8; 512]) -> bool {
    if buf[510] != 0x55 || buf[511] != 0xAA { return false; }
    &buf[0..8] == b"EFI PART"
}

#[no_mangle]
pub extern "C" fn main(argc: u64, argv: *const *const u8) -> i32 {
    let dev = if argc > 1 {
        unsafe {
            let ptr = *argv.add(1);
            if ptr.is_null() { "/dev/sda" }
            else {
                match core::ffi::CStr::from_ptr(ptr as *const i8).to_str() {
                    Ok(s) => s,
                    Err(_) => "/dev/sda",
                }
            }
        }
    } else { "/dev/sda" };

    let cpath = alloc::ffi::CString::new(dev).unwrap();
    let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 0);
    if (fd as i64) < 0 {
        let msg = alloc::format!("fdisk: cannot open {}\n", dev);
        skyos_libc::syscall::write(2, msg.as_bytes());
        return 1;
    }

    let mut size: u64 = 0;
    skyos_libc::syscall::ioctl(fd, BLKGETSIZE64, &mut size as *mut u64 as *mut u8);

    let header = alloc::format!(
        "Disk {}: {} bytes, {} sectors\n",
        dev, size, size / 512
    );
    skyos_libc::syscall::write(1, header.as_bytes());

    let mut mbr = [0u8; 512];
    if !read_sectors(fd, 0, 1, &mut mbr) {
        skyos_libc::syscall::write(2, b"fdisk: failed to read MBR\n");
        skyos_libc::syscall::close(fd);
        return 1;
    }

    if parse_gpt(&mbr) {
        let mut gpt_header = [0u8; 512];
        if read_sectors(fd, 1, 1, &mut gpt_header) && &gpt_header[0..8] == b"EFI PART" {
            skyos_libc::syscall::write(1, b"Disklabel type: gpt\n");
            let entry_lba = u64::from_le_bytes(gpt_header[72..80].try_into().unwrap());
            let num_entries = u32::from_le_bytes(gpt_header[80..84].try_into().unwrap());
            let entry_size = u32::from_le_bytes(gpt_header[84..88].try_into().unwrap());
            let entries_per_sector = 512 / entry_size as usize;
            let mut ent_buf = [0u8; 512];
            let mut shown = 0;
            for i in 0..num_entries.min(128) as usize {
                if i % entries_per_sector == 0 {
                    if !read_sectors(fd, entry_lba + (i as u64 / entries_per_sector as u64), 1, &mut ent_buf) { break; }
                }
                let off = (i % entries_per_sector) * entry_size as usize;
                let type_guid: [u8; 16] = unsafe { core::ptr::read_unaligned(ent_buf.as_ptr().add(off) as *const [u8; 16]) };
                if type_guid == [0u8; 16] { continue; }
                let start = u64::from_le_bytes(ent_buf[off+32..off+40].try_into().unwrap());
                let end = u64::from_le_bytes(ent_buf[off+40..off+48].try_into().unwrap());
                let num = end.wrapping_sub(start).wrapping_add(1);
                let line = alloc::format!("  Partition {}: LBA {} - {} ({} sectors)\n", i + 1, start, end, num);
                skyos_libc::syscall::write(1, line.as_bytes());
                shown += 1;
            }
            if shown == 0 {
                skyos_libc::syscall::write(1, b"  No partitions found\n");
            }
        } else {
            skyos_libc::syscall::write(1, b"Disklabel type: gpt (header unreadable)\n");
        }
    } else {
        skyos_libc::syscall::write(1, b"Disklabel type: mbr\n");
        let parts = parse_mbr(&mbr);
        if parts.is_empty() {
            skyos_libc::syscall::write(1, b"  No partitions found\n");
        }
        for (num, start, count, type_name) in &parts {
            let line = alloc::format!(
                "  Partition {}: LBA {} - {} ({} sectors, {})\n",
                num, start, start + count - 1, count, type_name
            );
            skyos_libc::syscall::write(1, line.as_bytes());
        }
    }

    skyos_libc::syscall::close(fd);
    0
}
