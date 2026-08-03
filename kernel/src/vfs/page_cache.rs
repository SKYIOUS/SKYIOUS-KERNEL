use hashbrown::HashMap;
use alloc::vec::Vec;
use alloc::sync::Arc;
use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;

pub const PAGE_SIZE: usize = 4096;
// ponytail: FIFO eviction, LRU if perf matters
const MAX_CACHED_PAGES: usize = 4096;

pub struct Page {
    pub data: [u8; PAGE_SIZE],
    #[allow(dead_code)]
    pub dirty: bool,
}

pub struct PageCache {
    /// Maps (inode_id, page_index) to Page
    pages: Mutex<HashMap<(u64, u64), Arc<Mutex<Page>>>>,
}

impl PageCache {
    pub fn new() -> Self {
        PageCache {
            pages: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_page(&self, ino: u64, index: u64) -> Option<Arc<Mutex<Page>>> {
        self.pages.lock().get(&(ino, index)).cloned()
    }

    pub fn insert_page(&self, ino: u64, index: u64, data: [u8; PAGE_SIZE]) -> Arc<Mutex<Page>> {
        let page = Arc::new(Mutex::new(Page { data, dirty: false }));
        let mut pages = self.pages.lock();
        if pages.len() >= MAX_CACHED_PAGES {
            if let Some(key) = pages.keys().next().cloned() {
                pages.remove(&key);
            }
        }
        pages.insert((ino, index), page.clone());
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

lazy_static! {
    pub static ref GLOBAL_PAGE_CACHE: PageCache = PageCache::new();
}
