// ponytail: single sector read test, no framework
use crate::drivers::block::BLOCK_DEVICES;

pub fn test_pata_mbr_sig() -> Result<(), &'static str> {
    let devices = BLOCK_DEVICES.lock();
    if devices.is_empty() {
        return Err("no block device to test");
    }
    let dev = devices[0].clone();
    drop(devices);

    let mut buf = [0u8; 512];
    dev.lock().read_sector(0, &mut buf).map_err(|_| "read failed")?;

    if buf[510] == 0x55 && buf[511] == 0xAA {
        crate::serial_write("[TEST] pata_mbr_sig: PASS (MBR signature valid)\n");
        Ok(())
    } else {
        crate::serial_write("[TEST] pata_mbr_sig: OK (no MBR, non-partitioned disk)\n");
        // Non-partitioned disk is not an error
        Ok(())
    }
}
