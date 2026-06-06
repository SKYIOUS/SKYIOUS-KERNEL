use x86_64::instructions::port::Port;
use core::sync::atomic::Ordering;

pub const VBE_DISPI_IOPORT_INDEX: u16 = 0x01CE;
pub const VBE_DISPI_IOPORT_DATA: u16 = 0x01CF;

pub const VBE_DISPI_INDEX_ID: u16 = 0;
pub const VBE_DISPI_INDEX_XRES: u16 = 1;
pub const VBE_DISPI_INDEX_YRES: u16 = 2;
pub const VBE_DISPI_INDEX_BPP: u16 = 3;
pub const VBE_DISPI_INDEX_ENABLE: u16 = 4;
pub const VBE_DISPI_INDEX_BANK: u16 = 5;
pub const VBE_DISPI_INDEX_VIRT_WIDTH: u16 = 6;
pub const VBE_DISPI_INDEX_VIRT_HEIGHT: u16 = 7;
pub const VBE_DISPI_INDEX_X_OFFSET: u16 = 8;
pub const VBE_DISPI_INDEX_Y_OFFSET: u16 = 9;

pub const VBE_DISPI_DISABLED: u16 = 0x00;
pub const VBE_DISPI_ENABLED: u16 = 0x01;
pub const VBE_DISPI_LFB_ENABLED: u16 = 0x40;

pub struct Bga {
    pub frame_buffer_phys: usize,
    pub width: u16,
    pub height: u16,
    pub bpp: u16,
}

impl Bga {
    pub fn new(fb_phys: usize) -> Self {
        Bga {
            frame_buffer_phys: fb_phys,
            width: 800,
            height: 600,
            bpp: 32,
        }
    }

    fn write_reg(index: u16, data: u16) {
        let mut index_port = Port::new(VBE_DISPI_IOPORT_INDEX);
        let mut data_port = Port::new(VBE_DISPI_IOPORT_DATA);
        unsafe {
            index_port.write(index);
            data_port.write(data);
        }
    }

        fn _read_reg(index: u16) -> u16 {
        let mut index_port = Port::new(VBE_DISPI_IOPORT_INDEX);
        let mut data_port = Port::new(VBE_DISPI_IOPORT_DATA);
        unsafe {
            index_port.write(index);
            data_port.read()
        }
    }

    pub fn init(&self) {
        crate::println!("BGA: Initializing {}x{} {}bpp", self.width, self.height, self.bpp);
        
        Self::write_reg(VBE_DISPI_INDEX_ENABLE, VBE_DISPI_DISABLED);
        Self::write_reg(VBE_DISPI_INDEX_XRES, self.width);
        Self::write_reg(VBE_DISPI_INDEX_YRES, self.height);
        Self::write_reg(VBE_DISPI_INDEX_BPP, self.bpp);
        Self::write_reg(VBE_DISPI_INDEX_ENABLE, VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED);
        
        // Map and Clear the frame buffer
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap_or(&0);
        let virt_fb = (offset as usize + self.frame_buffer_phys) as *mut u32;
        
        crate::println!("BGA: Framebuffer at virt 0x{:x}", virt_fb as usize);
        
        crate::drivers::graphics::FRAMEBUFFER.store(virt_fb, Ordering::Relaxed);
        
        // Fill with a nice color (Deep Blue)
        let color: u32 = 0x001A237E;
        let total_pixels = (self.width as u32 * self.height as u32) as usize;
        unsafe {
            for i in 0..total_pixels {
                *virt_fb.add(i) = color;
            }
        }
    }
}
