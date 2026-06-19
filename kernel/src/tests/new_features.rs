use crate::selftest;

pub fn test_entropy() -> Result<(), &'static str> {
    let e1 = crate::crypto::GLOBAL_ENTROPY.get_u64();
    let e2 = crate::crypto::GLOBAL_ENTROPY.get_u64();
    if e1 == e2 {
        return Err("Entropy harvester returned duplicate values");
    }
    if e1 == 0 && e2 == 0 {
        return Err("Entropy harvester returned all zeros");
    }
    Ok(())
}

pub fn test_page_cache() -> Result<(), &'static str> {
    use crate::vfs::page_cache::GLOBAL_PAGE_CACHE;
    let ino = 9999;
    let data = [0xAAu8; 4096];
    GLOBAL_PAGE_CACHE.insert_page(ino, 0, data);

    let cached = GLOBAL_PAGE_CACHE.get_page(ino, 0).ok_or("Page not found in cache")?;
    if cached.lock().data[0] != 0xAA {
        return Err("Cached data mismatch");
    }

    GLOBAL_PAGE_CACHE.evict_inode(ino);
    if GLOBAL_PAGE_CACHE.get_page(ino, 0).is_some() {
        return Err("Page still in cache after eviction");
    }

    Ok(())
}

pub fn register_all() {
    selftest::register("entropy::robust_harvester", test_entropy);
    selftest::register("vfs::page_cache_basic", test_page_cache);
}
