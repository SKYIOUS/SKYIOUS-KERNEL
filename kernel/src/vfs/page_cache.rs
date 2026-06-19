use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;

pub const PAGE_SIZE: usize = 4096;

pub struct Page {
    pub data: [u8; PAGE_SIZE],
    #[allow(dead_code)]
    pub dirty: bool,
}

pub struct PageCache {
    /// Maps (inode_id, page_index) to Page
    pages: Mutex<BTreeMap<(u64, u64), Arc<Mutex<Page>>>>,
}

impl PageCache {
    pub const fn new() -> Self {
        PageCache {
            pages: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn get_page(&self, ino: u64, index: u64) -> Option<Arc<Mutex<Page>>> {
        self.pages.lock().get(&(ino, index)).cloned()
    }

    pub fn insert_page(&self, ino: u64, index: u64, data: [u8; PAGE_SIZE]) -> Arc<Mutex<Page>> {
        let page = Arc::new(Mutex::new(Page { data, dirty: false }));
        self.pages.lock().insert((ino, index), page.clone());
        page
    }

    #[allow(dead_code)]
    pub fn mark_dirty(&self, ino: u64, index: u64) {
        if let Some(page) = self.pages.lock().get(&(ino, index)) {
            page.lock().dirty = true;
        }
    }

    #[allow(dead_code)]
    pub fn evict_inode(&self, ino: u64) {
        let mut pages = self.pages.lock();
        let keys: Vec<_> = pages.keys().filter(|(i, _)| *i == ino).cloned().collect();
        for k in keys {
            pages.remove(&k);
        }
    }
}

pub static GLOBAL_PAGE_CACHE: PageCache = PageCache::new();
