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

fn format_ext2(fd: u64, total_sectors: u64, label: &str) -> bool {
    let total_blocks = total_sectors / 2;
    let block_size: u32 = 1024;
    let blocks_per_group: u32 = 8192;
    let inodes_per_group: u32 = 2048;
    let inode_size: u16 = 128;
    let group_count = (total_blocks + blocks_per_group as u64 - 1) / blocks_per_group as u64;

    // Superblock (sector 2 for block_size=1024)
    let mut sb = [0u8; 1024];
    let mut sb_data = [0u8; 1024];
    // Ext2 superblock fields
    let le = |v: u32| v.to_le_bytes();
    sb_data[0..4].copy_from_slice(&le(total_blocks as u32));       // s_inodes_count
    sb_data[4..8].copy_from_slice(&le(total_blocks as u32));       // s_blocks_count
    sb_data[8..12].copy_from_slice(&le(0));                         // s_r_blocks_count
    sb_data[12..16].copy_from_slice(&le(total_blocks as u32));      // s_free_blocks_count
    sb_data[16..20].copy_from_slice(&le(inodes_per_group * group_count as u32)); // s_free_inodes_count
    sb_data[20..24].copy_from_slice(&le(0));                        // s_first_data_block
    sb_data[24..28].copy_from_slice(&le(0));                        // s_log_block_size (0 = 1024)
    sb_data[28..32].copy_from_slice(&le(0));                        // s_log_frag_size
    sb_data[32..36].copy_from_slice(&le(blocks_per_group));         // s_blocks_per_group
    sb_data[36..40].copy_from_slice(&le(blocks_per_group));         // s_frags_per_group
    sb_data[40..44].copy_from_slice(&le(inodes_per_group));         // s_inodes_per_group
    sb_data[44..48].copy_from_slice(&le(1));                        // s_mtime
    sb_data[48..52].copy_from_slice(&le(1));                        // s_wtime
    sb_data[52..54].copy_from_slice(&[0, 0]);                       // s_mnt_count
    sb_data[54..56].copy_from_slice(&[0xFF, 0xFF]);                 // s_max_mnt_count
    sb_data[56..58].copy_from_slice(&[0x53, 0xEF]);                 // s_magic
    sb_data[60..62].copy_from_slice(&[1, 0]);                       // s_rev_level
    sb_data[62..64].copy_from_slice(&[0, 0]);                       // s_errors
    sb_data[64..68].copy_from_slice(&[0; 4]);                        // s_lastcheck
    sb_data[68..72].copy_from_slice(&[0; 4]);                        // s_checkinterval
    sb_data[72..76].copy_from_slice(&[0; 4]);                        // s_creator_os (0 = Linux)
    sb_data[76..80].copy_from_slice(&[0, 0, 0, 0]);                 // s_rev_level
    sb_data[80..82].copy_from_slice(&[0, 0]);                        // s_def_resuid
    sb_data[82..84].copy_from_slice(&[0, 0]);                        // s_def_resgid
    sb_data[84..88].copy_from_slice(&le(11));                        // s_first_ino
    sb_data[88..90].copy_from_slice(&inode_size.to_le_bytes());      // s_inode_size
    // Volume name
    let label_bytes = label.as_bytes();
    let max_label = core::cmp::min(label_bytes.len(), 16);
    sb_data[120..120+max_label].copy_from_slice(&label_bytes[..max_label]);

    sb[0..1024].copy_from_slice(&sb_data);

    // Write superblock
    let mut sector_buf = vec![0u8; 512];
    sector_buf.copy_from_slice(&sb[0..512]);
    if !write_sectors(fd, 2, 1, &sector_buf) { return false; }
    sector_buf.copy_from_slice(&sb[512..1024]);
    if !write_sectors(fd, 3, 1, &sector_buf) { return false; }

    // Block group descriptor (sector 2 for 1024 block size, after superblock)
    let mut gd_buf = vec![0u8; 512];
    let gd_count = core::cmp::min(group_count as usize, 16);
    for g in 0..gd_count {
        let off = g * 32;
        gd_buf[off..off+4].copy_from_slice(&le((1 + g as u32) * 2)); // bg_block_bitmap (after superblock + GD)
        gd_buf[off+4..off+8].copy_from_slice(&le((1 + g as u32) * 2 + 1)); // bg_inode_bitmap
        gd_buf[off+8..off+12].copy_from_slice(&le((1 + g as u32) * 2 + 2)); // bg_inode_table
        gd_buf[off+12..off+14].copy_from_slice(&(blocks_per_group as u16).to_le_bytes()); // bg_free_blocks_count
        gd_buf[off+14..off+16].copy_from_slice(&(inodes_per_group as u16).to_le_bytes()); // bg_free_inodes_count
    }
    // Write GD (starts at block 1 = sector 2 for 1024 block size)
    let gd_sector = (block_size as u64) / 512;
    if !write_sectors(fd, gd_sector, 1, &gd_buf) { return false; }

    // Block bitmap (all zeros = all free)
    let bitmap_sector = gd_sector + 1;
    let bitmap = vec![0u8; 512];
    if !write_sectors(fd, bitmap_sector, 1, &bitmap) { return false; }

    // Inode bitmap (all zeros = all free)
    let inode_bitmap_sector = bitmap_sector + 1;
    if !write_sectors(fd, inode_bitmap_sector, 1, &bitmap) { return false; }

    // Root inode (inode 2) at inode table
    let itable_sector = inode_bitmap_sector + 1;
    let mut inode = [0u8; 128];
    inode[0..2].copy_from_slice(&[0x41, 0xED]); // i_mode: directory, perms 0755
    inode[2..4].copy_from_slice(&[0, 0]);         // i_uid
    inode[4..8].copy_from_slice(&le(1024));        // i_size
    inode[12..14].copy_from_slice(&[2, 0]);        // i_links_count (., ..)
    inode[40..44].copy_from_slice(&le(2));         // i_block[0]: block containing root dir

    let mut inode_sector_buf = vec![0u8; 512];
    inode_sector_buf[..128].copy_from_slice(&inode);
    if !write_sectors(fd, itable_sector, 1, &inode_sector_buf) { return false; }

    // Root directory block
    let root_block_sector = itable_sector + 1;
    let mut root_dir = vec![0u8; 512];
    root_dir[0..4].copy_from_slice(&le(2));          // inode 2 = .
    root_dir[4..6].copy_from_slice(&le(12));          // rec_len
    root_dir[6..8].copy_from_slice(&[1, 0]);           // name_len=1, type=DIR
    root_dir[8..9].copy_from_slice(b".");
    root_dir[12..16].copy_from_slice(&le(2));          // inode 2 = ..
    root_dir[16..18].copy_from_slice(&le(12 + 12 + 2)); // rec_len to end
    root_dir[18..20].copy_from_slice(&[2, 0]);         // name_len=2, type=DIR
    root_dir[20..22].copy_from_slice(b"..");
    // Fill rest with zeros
    if !write_sectors(fd, root_block_sector, 1, &root_dir) { return false; }

    true
}

fn print_usage() {
    skyos_libc::syscall::write(2, b"Usage: mkfs.ext2 [-L label] <device>\n");
}

#[no_mangle]
pub extern "C" fn main(argc: u64, argv: *const *const u8) -> i32 {
    if argc < 2 { print_usage(); return 1; }

    let mut device: Option<&str> = None;
    let mut label = "";

    let mut i = 1;
    while (i as u64) < argc {
        let arg = unsafe {
            let ptr = *argv.add(i);
            if ptr.is_null() { break; }
            core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
        };
        match arg {
            "-L" => {
                i += 1;
                label = unsafe {
                    let ptr = *argv.add(i);
                    core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
                };
            }
            _ => device = Some(arg),
        }
        i += 1;
    }

    let dev = match device { Some(d) => d, None => { print_usage(); return 1; } };
    let cpath = alloc::ffi::CString::new(dev).unwrap();
    let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 1);
    if (fd as i64) < 0 {
        let msg = alloc::format!("mkfs.ext2: cannot open {}\n", dev);
        skyos_libc::syscall::write(2, msg.as_bytes());
        return 1;
    }

    let mut size: u64 = 0;
    skyos_libc::syscall::ioctl(fd, BLKGETSIZE64, &mut size as *mut u64 as *mut u8);
    if size < 1024 * 1024 {
        skyos_libc::syscall::write(2, b"mkfs.ext2: device too small (min 1 MB)\n");
        skyos_libc::syscall::close(fd);
        return 1;
    }

    let sectors = size / 512;
    let msg = alloc::format!("mkfs.ext2: Creating ext2 on {} ({} sectors)...\n", dev, sectors);
    skyos_libc::syscall::write(1, msg.as_bytes());

    if format_ext2(fd, sectors, label) {
        skyos_libc::syscall::write(1, b"mkfs.ext2: Done.\n");
        skyos_libc::syscall::close(fd);
        0
    } else {
        skyos_libc::syscall::write(2, b"mkfs.ext2: Failed to create filesystem\n");
        skyos_libc::syscall::close(fd);
        1
    }
}
