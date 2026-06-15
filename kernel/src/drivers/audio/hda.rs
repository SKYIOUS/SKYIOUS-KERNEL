//! Intel High Definition Audio (HDA) Driver with PCM playback

use volatile::Volatile;
use alloc::vec::Vec;
use core::ptr;
use x86_64::VirtAddr;

macro_rules! hda_println {
    ($($arg:tt)*) => {{
        let s = alloc::format!($($arg)*);
        crate::serial_write(&s);
        crate::serial_write("\n");
        crate::print!("{}", s);
        crate::print!("\n");
    }};
}

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: u8 = 2;
const BITS_PER_SAMPLE: u8 = 16;
const BLOCK_SIZE: usize = 8192;
const NUM_BUFFERS: u32 = 4;

#[repr(C)]
pub struct HdaRegisters {
    pub gcap:    Volatile<u16>,
    pub vmin:    Volatile<u8>,
    pub vmaj:    Volatile<u8>,
    pub outpay:  Volatile<u16>,
    pub inpay:   Volatile<u16>,
    pub gctl:    Volatile<u32>,
    pub wakeen:  Volatile<u16>,
    pub statests:Volatile<u16>,
    pub gsts:    Volatile<u16>,
    _reserved1:  [u8; 6],
    pub outstrmpay: Volatile<u16>,
    pub instrmpay:  Volatile<u16>,
    _reserved2:  [u32; 4],
    pub intctl:  Volatile<u32>,
    pub intsts:  Volatile<u32>,
}

#[repr(C)]
pub struct HdaStreamDesc {
    _reserved0: [u32; 2],
    pub ctl:    Volatile<u32>,
    pub status: Volatile<u32>,
    pub fmt:    Volatile<u32>,
    _reserved1: [u32; 1],
    pub bdpl:   Volatile<u32>,
    pub bdph:   Volatile<u32>,
    pub lvi:    Volatile<u32>,
    pub fifos:  Volatile<u32>,
    pub fiffmt: Volatile<u32>,
    _reserved2: [u32; 4],
}

const CORB_SIZE: usize = 256;
const RIRB_SIZE: usize = 512; // 256 entries * 2 u32 per entry

const SD_OFFSET: usize = 0x80;
const SD_STRIDE: usize = 0x20;

unsafe impl Send for HdaController {}
unsafe impl Sync for HdaController {}

pub struct HdaController {
    base_addr: usize,
    audio_buffers: Vec<*mut u8>,
    corb_buf: Option<(*mut u32, usize)>,
    rirb_buf: Option<(*mut u32, usize)>,
    #[allow(dead_code)]
    inflight_verbs: Vec<u32>,
    out_nid: Option<u8>,
    pin_nid: Option<u8>,
    afg_nid: Option<u8>,
    stream_running: bool,
}

impl HdaController {
    pub fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            audio_buffers: Vec::new(),
            corb_buf: None,
            rirb_buf: None,
            inflight_verbs: Vec::new(),
            out_nid: None,
            pin_nid: None,
            afg_nid: None,
            stream_running: false,
        }
    }

    fn regs(&self) -> &mut HdaRegisters {
        unsafe { &mut *(self.base_addr as *mut HdaRegisters) }
    }

    fn read_reg8(&self, off: usize) -> u8 {
        unsafe { core::ptr::read_volatile((self.base_addr + off) as *const u8) }
    }
    fn write_reg8(&self, off: usize, val: u8) {
        unsafe { core::ptr::write_volatile((self.base_addr + off) as *mut u8, val) }
    }
    fn read_reg16(&self, off: usize) -> u16 {
        unsafe { core::ptr::read_volatile((self.base_addr + off) as *const u16) }
    }
    fn write_reg16(&self, off: usize, val: u16) {
        unsafe { core::ptr::write_volatile((self.base_addr + off) as *mut u16, val) }
    }
    #[allow(dead_code)]
    fn read_reg32(&self, off: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base_addr + off) as *const u32) }
    }
    fn write_reg32(&self, off: usize, val: u32) {
        unsafe { core::ptr::write_volatile((self.base_addr + off) as *mut u32, val) }
    }

    // CORB MMIO offsets (from HDA spec, not the DMA buffer approach)
    const CORBWP: usize = 0x48;
    const CORBRP: usize = 0x4A;
    const CORBCTL: usize = 0x4C;
    const CORBSTS: usize = 0x4D;
    const CORBSIZE: usize = 0x4E;
    const RIRBWP: usize = 0x58;
    const RIRBCTL: usize = 0x5A;
    const RIRBSTS: usize = 0x5C;
    const RIRBSIZE: usize = 0x5D;

    fn corb_buf_mut(&self) -> &mut [u32] {
        let (ptr, _) = self.corb_buf.unwrap();
        unsafe { core::slice::from_raw_parts_mut(ptr, CORB_SIZE) }
    }

    fn rirb_buf(&self) -> &[u32] {
        let (ptr, _) = self.rirb_buf.unwrap();
        unsafe { core::slice::from_raw_parts(ptr, RIRB_SIZE) }
    }

    fn rirb_buf_mut(&mut self) -> &mut [u32] {
        let (ptr, _) = self.rirb_buf.unwrap();
        unsafe { core::slice::from_raw_parts_mut(ptr, RIRB_SIZE) }
    }

    fn send_verb(&mut self, codec_addr: u8, nid: u8, verb: u16, param: u16) -> u32 {
        let verb_data = ((codec_addr as u32) << 28)
            | ((nid as u32) << 20)
            | ((verb as u32) << 8)
            | (param as u32);

        // Clear CORBSTS before starting
        self.write_reg8(Self::CORBSTS, 1);

        // CORBOK (CORBSTS bit 0) may not be set by all emulators/hardware;
        // just proceed with submitting the verb

        let wp = self.read_reg8(Self::CORBWP) as usize;
        if wp >= 256 {
            hda_println!("HDA: CORBWP out of range: {}", wp);
            return 0;
        }
        self.corb_buf_mut()[wp] = verb_data;
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Flush cache line containing verb data so DMA engine sees it
        unsafe {
            core::arch::x86_64::_mm_clflush(
                (self.corb_buf.as_ref().unwrap().0.add(wp) as *const u32) as *const u8
            );
        }

        let new_wp = wp.wrapping_add(1) as u8;
        self.write_reg8(Self::CORBWP, new_wp);

        // Wait for response in RIRB (poll RIRBWP for change)
        // QEMU's HDA timer may take ~1ms to fire; use a generous timeout
        let start_rp = self.read_reg16(Self::RIRBWP) as usize & 0xFF;
        let mut timeout = 500_000_000u32;
        while timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;

            let rirbwp = self.read_reg16(Self::RIRBWP) as usize & 0xFF;
            if rirbwp != start_rp {
                // Response written at old RIRBWP, then incremented
                let rp = (rirbwp + 255) & 0xFF; // (rirbwp - 1) mod 256
                let response = self.rirb_buf()[rp * 2]; // entry = 2 u32, response at offset 0
                core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                unsafe {
                    core::arch::x86_64::_mm_clflush(
                        ((self.rirb_buf.as_ref().unwrap().0.add(rp * 2) as *const u32).add(1)) as *const u8
                    );
                }
                self.rirb_buf_mut()[rp * 2 + 1] = 0; // mark consumed
                self.write_reg8(Self::RIRBSTS, 1);
                return response;
            }
        }
        hda_println!("HDA: RIRB timeout after verb 0x{:08X} (start_rp={})", verb_data, start_rp);
        0
    }

    pub fn init(&mut self) {
        let (_vmaj, _vmin, num_out) = {
            let regs = self.regs();
            let vmaj = regs.vmaj.read();
            let vmin = regs.vmin.read();
            hda_println!("HDA: Controller at 0x{:x}, Version {}.{}", self.base_addr, vmaj, vmin);

            // Reset controller
            hda_println!("HDA: Resetting controller...");
            let mut gctl = regs.gctl.read();
            gctl &= !1;
            regs.gctl.write(gctl);
            while (regs.gctl.read() & 1) != 0 { core::hint::spin_loop(); }
            gctl |= 1;
            regs.gctl.write(gctl);
            while (regs.gctl.read() & 1) == 0 { core::hint::spin_loop(); }
            hda_println!("HDA: Controller reset done");

            regs.intctl.write(1 << 31);

            let gcap_val = regs.gcap.read();
        let num_out = ((gcap_val >> 12) & 0xF) as usize;
            hda_println!("HDA: Output streams: {}, GCAP=0x{:04X}", num_out, gcap_val);
            (vmaj, vmin, num_out)
        };
        if num_out == 0 { return; }

        // Allocate DMA buffers for CORB (256 * 4 = 1024 bytes) and RIRB (256 * 8 = 2048 bytes)
        let corb_layout = alloc::alloc::Layout::from_size_align(CORB_SIZE * 4, 4096).unwrap();
        let rirb_layout = alloc::alloc::Layout::from_size_align(RIRB_SIZE * 4, 4096).unwrap();
        let corb_ptr = unsafe { alloc::alloc::alloc(corb_layout) };
        let rirb_ptr = unsafe { alloc::alloc::alloc(rirb_layout) };
        if corb_ptr.is_null() || rirb_ptr.is_null() {
            hda_println!("HDA: Failed to allocate DMA buffers");
            return;
        }
        unsafe { ptr::write_bytes(corb_ptr, 0, CORB_SIZE * 4); }
        unsafe { ptr::write_bytes(rirb_ptr, 0, RIRB_SIZE * 4); }
        self.corb_buf = Some((corb_ptr as *mut u32, CORB_SIZE));
        self.rirb_buf = Some((rirb_ptr as *mut u32, RIRB_SIZE));

        let corb_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(corb_ptr)).unwrap();
        let rirb_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(rirb_ptr)).unwrap();
        hda_println!("HDA: CORB phys=0x{:016X}, RIRB phys=0x{:016X}", corb_phys.as_u64(), rirb_phys.as_u64());

        // Disable CORB/RIRB before configuring
        self.write_reg8(Self::CORBCTL, 0);
        self.write_reg8(Self::RIRBCTL, 0);
        self.write_reg8(Self::CORBSTS, 1);
        self.write_reg8(Self::RIRBSTS, 1);
        core::hint::spin_loop();

        // Set CORB size to 256 entries (2 = 256 entries)
        self.write_reg8(Self::CORBSIZE, 2);
        self.write_reg8(Self::RIRBSIZE, 2);
        core::hint::spin_loop();

        // Set CORB/RIRB base physical addresses
        self.write_reg32(0x40, corb_phys.as_u64() as u32); // CORBBASE low
        self.write_reg32(0x44, (corb_phys.as_u64() >> 32) as u32); // CORBBASE high
        self.write_reg32(0x50, rirb_phys.as_u64() as u32); // RIRBBASE low
        self.write_reg32(0x54, (rirb_phys.as_u64() >> 32) as u32); // RIRBBASE high

        // Reset CORB/RIRB pointers
        self.write_reg8(Self::CORBRP, 0x80); // CORB reset (bit 7)
        self.write_reg8(Self::CORBRP, 0); // Clear reset
        self.write_reg16(Self::RIRBWP, 0);

        // Wake codecs from D3
        self.regs().wakeen.write(0x0F);

        // Enable CORB and RIRB (set run bit) — use 8-bit writes per spec
        self.write_reg8(Self::CORBCTL, 1); // CORB run (bit 0)
        self.write_reg8(Self::RIRBCTL, 1); // RIRB run (bit 0)
        self.write_reg8(0x48, 0); // CORBWP = 0
        self.write_reg8(Self::CORBSTS, 1);
        self.write_reg8(Self::RIRBSTS, 1);
        core::hint::spin_loop();

        // Probe codecs
        let codecs_mask = (self.regs().statests.read() & 0x0F) as u8;
        hda_println!("HDA: Codec mask: 0x{:02x}", codecs_mask);
        if codecs_mask == 0 { return; }

        let mut afg_nid = 0u8;
        let mut out_nid = 0u8;
        let mut pin_nid = 0u8;

        for addr in 0..4 {
            if (codecs_mask & (1 << addr)) == 0 { continue; }

            let vendor = self.send_verb(addr, 0, 0xF00, 0);
            let rev = self.send_verb(addr, 0, 0xF00, 2);
            if vendor == 0 { continue; }

            let vendor_upper = (vendor >> 16) & 0xFFFF;
            let vendor_lower = vendor & 0xFFFF;
            hda_println!("HDA: Codec {}: 0x{:04X}:0x{:04X}, rev 0x{:02X}", addr, vendor_upper, vendor_lower, rev);

            let start_nid = self.send_verb(addr, 0, 0xF00, 4) as u8;
            let total_nids = self.send_verb(addr, 0, 0xF00, 5) as u8;
            hda_println!("HDA:   NID range: {}-{}", start_nid, start_nid + total_nids - 1);

            for nid in start_nid..start_nid + total_nids {
                let caps = self.send_verb(addr, nid, 0xF00, 9);
                let wtype = ((caps >> 20) & 0xF) as u8;
                let wtype_name = match wtype {
                    0x0 => "Audio Output",
                    0x1 => "Audio Input",
                    0x2 => "Audio Mixer",
                    0x3 => "Audio Selector",
                    0x4 => "Pin Complex",
                    0x5 => "Power Widget",
                    0x6 => "Volume Widget",
                    0x7 => "Beep Generator",
                    _ => "Unknown",
                };
                hda_println!("HDA:   NID 0x{:02X}: {} (type 0x{:X})", nid, wtype_name, wtype);

                match wtype {
                    0x0 => { if out_nid == 0 { out_nid = nid; } }
                    0x4 => { if pin_nid == 0 { pin_nid = nid; } }
                    _ => {}
                }
            }

            // AFG is always at NID 1 (or start_nid)
            afg_nid = start_nid;
        }

        if out_nid == 0 { hda_println!("HDA: No output converter found"); return; }
        if pin_nid == 0 { hda_println!("HDA: No pin complex found"); return; }

        hda_println!("HDA: AFG=0x{:02X}, Output=0x{:02X}, Pin=0x{:02X}", afg_nid, out_nid, pin_nid);

        self.out_nid = Some(out_nid);
        self.pin_nid = Some(pin_nid);
        self.afg_nid = Some(afg_nid);

        // Set initial volume to 75%
        self.set_volume(75);

        // Generate test tone (440Hz sine, 16-bit stereo)
        // Using sine table (256-entry, 16-bit signed)
        let sine_table: [i16; 256] = [
            0, 804, 1608, 2410, 3212, 4011, 4808, 5602, 6393, 7179, 7962, 8739, 9512, 10278, 11039, 11793,
            12540, 13279, 14010, 14732, 15446, 16151, 16846, 17531, 18205, 18868, 19520, 20160, 20788, 21403, 22005, 22595,
            23170, 23732, 24279, 24812, 25330, 25833, 26320, 26791, 27246, 27684, 28106, 28511, 28899, 29269, 29622, 29957,
            30274, 30572, 30852, 31114, 31357, 31581, 31786, 31972, 32138, 32285, 32413, 32522, 32610, 32679, 32729, 32758,
            -32768, -32758, -32729, -32679, -32610, -32522, -32413, -32285, -32138, -31972, -31786, -31581, -31357, -31114, -30852, -30572,
            -30274, -29957, -29622, -29269, -28899, -28511, -28106, -27684, -27246, -26791, -26320, -25833, -25330, -24812, -24279, -23732,
            -23170, -22595, -22005, -21403, -20788, -20160, -19520, -18868, -18205, -17531, -16846, -16151, -15446, -14732, -14010, -13279,
            -12540, -11793, -11039, -10278, -9512, -8739, -7962, -7179, -6393, -5602, -4808, -4011, -3212, -2410, -1608, -804,
            0, 804, 1608, 2410, 3212, 4011, 4808, 5602, 6393, 7179, 7962, 8739, 9512, 10278, 11039, 11793,
            12540, 13279, 14010, 14732, 15446, 16151, 16846, 17531, 18205, 18868, 19520, 20160, 20788, 21403, 22005, 22595,
            23170, 23732, 24279, 24812, 25330, 25833, 26320, 26791, 27246, 27684, 28106, 28511, 28899, 29269, 29622, 29957,
            30274, 30572, 30852, 31114, 31357, 31581, 31786, 31972, 32138, 32285, 32413, 32522, 32610, 32679, 32729, 32758,
            -32768, -32758, -32729, -32679, -32610, -32522, -32413, -32285, -32138, -31972, -31786, -31581, -31357, -31114, -30852, -30572,
            -30274, -29957, -29622, -29269, -28899, -28511, -28106, -27684, -27246, -26791, -26320, -25833, -25330, -24812, -24279, -23732,
            -23170, -22595, -22005, -21403, -20788, -20160, -19520, -18868, -18205, -17531, -16846, -16151, -15446, -14732, -14010, -13279,
            -12540, -11793, -11039, -10278, -9512, -8739, -7962, -7179, -6393, -5602, -4808, -4011, -3212, -2410, -1608, -804,
        ];

        let total_frames = NUM_BUFFERS as usize * BLOCK_SIZE / (CHANNELS as usize * 2);
        let mut samples = alloc::vec![0u8; NUM_BUFFERS as usize * BLOCK_SIZE];
        let phase_step = (440 * 65536) / (SAMPLE_RATE / 64) as u32; // 64x oversampled phase
        let mut phase: u32 = 0;

        for i in 0..total_frames {
            let idx = ((phase >> 8) & 0xFF) as usize;
            let sample = sine_table[idx].to_le_bytes();
            let off = i * CHANNELS as usize * 2;
            if off + 3 < samples.len() {
                samples[off] = sample[0];
                samples[off + 1] = sample[1];
                samples[off + 2] = sample[0];
                samples[off + 3] = sample[1];
            }
            phase = phase.wrapping_add(phase_step);
        }

        // Set up output stream on SDI 0
        self.setup_output_stream(0, &samples);

        // Program codec: set converter format (16-bit PCM)
        let _fmt_codec = ((BITS_PER_SAMPLE as u16 - 1) << 4) | (CHANNELS as u16 - 1);
        self.send_verb(0, out_nid, 0x200, 0x20); // stream tag=2 (matches SD), channel=0
        self.send_verb(0, out_nid, 0xA00, 0x11); // format: 16-bit stereo
        self.send_verb(0, out_nid, 0x300, 0x8000); // unmute output amp
        self.send_verb(0, afg_nid, 0x300, 0x8000); // unmute AFG

        // Enable pin output
        self.send_verb(0, pin_nid, 0x700, 0x40); // PIN_OUT = bit 6
        self.send_verb(0, pin_nid, 0x700, 0x42); // PIN_OUT + EAPD

        hda_println!("HDA: Audio playback started (440Hz test tone)");
    }

    fn setup_output_stream(&mut self, index: usize, data: &[u8]) {
        // Use raw pointer to avoid borrow conflicts
        let sd_ptr = (self.base_addr + SD_OFFSET + index * SD_STRIDE) as *mut HdaStreamDesc;
        let sd = unsafe { &mut *sd_ptr };

        // Reset stream
        sd.ctl.write(0);
        sd.status.write(1);
        core::hint::spin_loop();

        let num_buffers = NUM_BUFFERS;
        let block_size = BLOCK_SIZE;
        let bdl_entries = num_buffers as usize;

        let bdl_size = bdl_entries * 16;
        let bdl = unsafe {
            let ptr = alloc::alloc::alloc(
                alloc::alloc::Layout::from_size_align(bdl_size, 128).unwrap());
            if ptr.is_null() { return; }
            core::slice::from_raw_parts_mut(ptr, bdl_size)
        };

        let mut bufs = Vec::new();

        for i in 0..bdl_entries {
            let start = i * block_size;
            let end = (start + block_size).min(data.len());
            let buf_len = end - start;

            let buf = unsafe {
                let p = alloc::alloc::alloc(
                    alloc::alloc::Layout::from_size_align(block_size, 4096).unwrap());
                if p.is_null() { return; }
                core::ptr::copy_nonoverlapping(data.as_ptr().add(start), p, buf_len);
                p
            };
            bufs.push(buf);

            let buf_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(buf)).unwrap();

            let entry = unsafe { &mut *(bdl.as_mut_ptr().add(i * 16) as *mut [u32; 4]) };
            entry[0] = buf_phys.as_u64() as u32;
            entry[1] = (buf_phys.as_u64() >> 32) as u32;
            entry[2] = block_size as u32;
            entry[3] = 1;
        }

        // Store buffers in self (sd borrow is now done)
        for buf in bufs { self.audio_buffers.push(buf); }

        let bdl_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(bdl.as_ptr())).unwrap();
        sd.bdpl.write(bdl_phys.as_u64() as u32);
        sd.bdph.write((bdl_phys.as_u64() >> 32) as u32);
        sd.lvi.write(num_buffers - 1);

        let fmt_val = ((BITS_PER_SAMPLE as u32 - 1) << 8) | ((CHANNELS as u32 - 1) & 0x1F);
        let fmt_reg = (SAMPLE_RATE << 16) | fmt_val;
        sd.fmt.write(fmt_reg);

        sd.ctl.write(0);
        core::hint::spin_loop();

        sd.status.write(1);
        sd.ctl.write((1 << 20) | 2);
        core::hint::spin_loop();

        hda_println!("HDA: Stream {} started: tag=1, {}Hz, {}ch, {}bit",
            index, SAMPLE_RATE, CHANNELS, BITS_PER_SAMPLE);
        self.stream_running = true;
    }

    /// Set output volume (0-100).
    pub fn set_volume(&mut self, percent: u8) {
        let out = match self.out_nid { Some(n) => n, None => return };
        let afg = match self.afg_nid { Some(n) => n, None => return };

        // HDA volume is a 7-bit value (0-127) in the amp verb
        // Verb 0x300: Set Amplifier Gain/Mute
        //   bit 7 = mute (0 = no mute, 1 = mute)
        //   bits 6:0 = gain (0x7F = max, 0 = -64dB)
        let vol_linear = if percent >= 100 { 0x7F } else { (percent as u16 * 127 / 100) as u8 };
        let amp_val = (vol_linear as u32) & 0x7F; // no mute bit

        // Set output amp (both left and right channels)
        self.send_verb(0, out, 0x300, amp_val as u16);
        // Set AFG output amp
        self.send_verb(0, afg, 0x300, (amp_val | 0x8000) as u16); // bit 15 = set both channels

        hda_println!("HDA: Volume set to {}% (amp val 0x{:02X})", percent, amp_val);
    }

    /// Stop audio stream and release buffers.
    pub fn stop(&mut self) {
        if !self.stream_running { return; }

        // Stop stream SDI 0
        let sd_ptr = (self.base_addr + SD_OFFSET) as *mut HdaStreamDesc;
        let sd = unsafe { &mut *sd_ptr };
        sd.ctl.write(0);
        sd.status.write(0);
        self.stream_running = false;

        // Mute output
        if let Some(out) = self.out_nid {
            self.send_verb(0, out, 0x300, 0x8080); // mute
        }

        // Free buffers (they are DMA-allocated)
        for buf in self.audio_buffers.drain(..) {
            if !buf.is_null() {
                unsafe {
                    alloc::alloc::dealloc(buf, alloc::alloc::Layout::from_size_align(BLOCK_SIZE, 4096).unwrap());
                }
            }
        }

        hda_println!("HDA: Stream stopped, buffers freed");
    }
}
