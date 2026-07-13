use crate::alloc::sync::Arc;
use crate::alloc::vec::Vec;
use crate::vfs::ramfs::Tmpfs;
use crate::vfs::{FileSystem, VfsNode};

fn new_tmpfs() -> (Tmpfs, Arc<dyn VfsNode>) {
    let fs = Tmpfs::new();
    let root = fs.root().expect("tmpfs root failed");
    (fs, root)
}

fn test_tmpfs_create_file() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("test.txt").map_err(|_| "create failed")?;
    if file.is_dir() { return Err("file reports is_dir"); }
    let found = root.find_child("test.txt").ok_or("find_child failed")?;
    if found.name() != "test.txt" { return Err("name mismatch"); }
    Ok(())
}

fn test_tmpfs_create_duplicate() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.create("a.txt").map_err(|_| "first create failed")?;
    if root.create("a.txt").is_ok() { return Err("duplicate create should fail"); }
    Ok(())
}

fn test_tmpfs_create_on_file() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("f.txt").map_err(|_| "create failed")?;
    if file.create("nested").is_ok() { return Err("create on file should fail"); }
    Ok(())
}

fn test_tmpfs_mkdir_duplicate() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.mkdir("d").map_err(|_| "first mkdir failed")?;
    if root.mkdir("d").is_ok() { return Err("duplicate mkdir should fail"); }
    Ok(())
}

fn test_tmpfs_mkdir_on_file() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("f.txt").map_err(|_| "create failed")?;
    if file.mkdir("sub").is_ok() { return Err("mkdir on file should fail"); }
    Ok(())
}

fn test_tmpfs_create_dir_fails() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    if root.create(".").is_ok() { return Err("create '.' should fail"); }
    if root.create("/").is_ok() { return Err("create '/' should fail"); }
    Ok(())
}

fn test_tmpfs_write_read() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("data.bin").map_err(|_| "create failed")?;
    let content = b"Hello Vahi Kernel!";
    file.write(content).map_err(|_| "write failed")?;
    let stat = file.stat().map_err(|_| "stat failed")?;
    if stat.st_size != content.len() as i64 { return Err("st_size mismatch"); }
    let data = file.read(4096).map_err(|_| "read failed")?;
    if data.as_slice() != content { return Err("read-back mismatch"); }
    Ok(())
}

fn test_tmpfs_mkdir_children() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.mkdir("subdir").map_err(|_| "mkdir failed")?;
    let dir = root.find_child("subdir").ok_or("subdir not found")?;
    if !dir.is_dir() { return Err("subdir is not a dir"); }
    let stat = dir.stat().map_err(|_| "stat failed")?;
    if stat.st_mode & 0xF000 != 0x4000 { return Err("not directory mode"); }
    let children = root.children().map_err(|_| "children failed")?;
    let names: Vec<crate::alloc::string::String> = children.iter().map(|c| c.name()).collect();
    if !names.iter().any(|n| n == "subdir") { return Err("subdir not in children"); }
    Ok(())
}

fn test_tmpfs_rename() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.create("old.txt").map_err(|_| "create failed")?;
    root.rename("old.txt", "new.txt").map_err(|_| "rename failed")?;
    if root.find_child("old.txt").is_some() { return Err("old name still exists"); }
    root.find_child("new.txt").ok_or("new name not found")?;
    Ok(())
}

fn test_tmpfs_rename_nonexistent() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    if root.rename("nosuch", "new.txt").is_ok() { return Err("rename nonexistent should fail"); }
    Ok(())
}

fn test_tmpfs_rename_to_existing() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.create("a.txt").map_err(|_| "create a failed")?;
    root.create("b.txt").map_err(|_| "create b failed")?;
    if root.rename("a.txt", "b.txt").is_ok() { return Err("rename to existing should fail"); }
    Ok(())
}

fn test_tmpfs_rename_on_file() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("f.txt").map_err(|_| "create failed")?;
    if file.rename("x", "y").is_ok() { return Err("rename on file should fail"); }
    Ok(())
}

fn test_tmpfs_truncate() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("trunc.txt").map_err(|_| "create failed")?;
    file.write(b"1234567890").map_err(|_| "write failed")?;
    file.truncate(5).map_err(|_| "truncate failed")?;
    let stat = file.stat().map_err(|_| "stat failed")?;
    if stat.st_size != 5 { return Err("size should be 5 after truncate"); }
    let data = file.read(10).map_err(|_| "read failed")?;
    if data.len() != 5 { return Err("read length should be 5"); }
    if &data[..] != b"12345" { return Err("content mismatch after truncate"); }
    Ok(())
}

fn test_tmpfs_truncate_same() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("keep.txt").map_err(|_| "create failed")?;
    file.write(b"data").map_err(|_| "write failed")?;
    file.truncate(4).map_err(|_| "truncate to same len failed")?;
    let stat = file.stat().map_err(|_| "stat failed")?;
    if stat.st_size != 4 { return Err("size should remain 4"); }
    Ok(())
}

fn test_tmpfs_truncate_zero() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("z.txt").map_err(|_| "create failed")?;
    file.write(b"data").map_err(|_| "write failed")?;
    file.truncate(0).map_err(|_| "truncate to zero failed")?;
    let data = file.read(10).map_err(|_| "read failed")?;
    if !data.is_empty() { return Err("should be empty after truncate to 0"); }
    Ok(())
}

fn test_tmpfs_unlink() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.create("delete.me").map_err(|_| "create failed")?;
    let before = root.children().map_err(|_| "children failed")?.len();
    root.unlink("delete.me").map_err(|_| "unlink failed")?;
    let after = root.children().map_err(|_| "children failed")?.len();
    if after + 1 != before { return Err("child count should decrease by 1"); }
    if root.find_child("delete.me").is_some() { return Err("found after unlink"); }
    Ok(())
}

fn test_tmpfs_unlink_nonexistent() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    if root.unlink("nosuch").is_ok() { return Err("unlink nonexistent should fail"); }
    Ok(())
}

fn test_tmpfs_unlink_on_file() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("f.txt").map_err(|_| "create failed")?;
    if file.unlink("child").is_ok() { return Err("unlink on file should fail"); }
    Ok(())
}

fn test_tmpfs_nested_create() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let sub = root.mkdir("sub").map_err(|_| "mkdir failed")?;
    let file = sub.create("inner.txt").map_err(|_| "create in subdir failed")?;
    if file.is_dir() { return Err("inner file reports is_dir"); }
    let found = sub.find_child("inner.txt").ok_or("find_child in subdir failed")?;
    if found.name() != "inner.txt" { return Err("name mismatch"); }
    Ok(())
}

fn test_tmpfs_symlink_readlink() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.symlink("mylink", "/target/path").map_err(|_| "symlink failed")?;
    let link = root.find_child("mylink").ok_or("link not found")?;
    let target = link.readlink().map_err(|_| "readlink failed")?;
    if target != "/target/path" { return Err("link target mismatch"); }
    Ok(())
}

fn test_tmpfs_chmod() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("perm.txt").map_err(|_| "create failed")?;
    file.chmod(0o700).map_err(|_| "chmod failed")?;
    let stat = file.stat().map_err(|_| "stat failed")?;
    if stat.st_mode & 0o777 != 0o700 { return Err("mode mismatch"); }
    Ok(())
}

fn test_tmpfs_chown() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("owner.txt").map_err(|_| "create failed")?;
    file.chown(1000, 100).map_err(|_| "chown failed")?;
    let stat = file.stat().map_err(|_| "stat failed")?;
    if stat.st_uid != 1000 { return Err("uid mismatch"); }
    if stat.st_gid != 100 { return Err("gid mismatch"); }
    Ok(())
}

fn test_tmpfs_write_overwrite() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("overwrite.txt").map_err(|_| "create failed")?;
    file.write(b"first write").map_err(|_| "first write failed")?;
    file.truncate(0).map_err(|_| "truncate failed")?;
    file.write(b"second").map_err(|_| "second write failed")?;
    let data = file.read(20).map_err(|_| "read failed")?;
    if data.as_slice() != b"second" { return Err("overwrite content mismatch"); }
    Ok(())
}

fn test_tmpfs_write_append() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    let file = root.create("append.txt").map_err(|_| "create failed")?;
    file.write(b"ab").map_err(|_| "first write failed")?;
    file.write(b"cd").map_err(|_| "second write failed")?;
    let data = file.read(10).map_err(|_| "read failed")?;
    if data.as_slice() != b"abcd" { return Err("append content mismatch"); }
    Ok(())
}

fn test_tmpfs_write_dir_fails() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    if root.write(b"data").is_ok() { return Err("write on root dir should fail"); }
    let sub = root.mkdir("sub").map_err(|_| "mkdir failed")?;
    if sub.write(b"data").is_ok() { return Err("write on subdir should fail"); }
    Ok(())
}

fn test_tmpfs_read_dir_fails() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    if root.read(10).is_ok() { return Err("read on root dir should fail"); }
    let sub = root.mkdir("sub").map_err(|_| "mkdir failed")?;
    if sub.read(10).is_ok() { return Err("read on subdir should fail"); }
    Ok(())
}

fn test_tmpfs_multiple_files() -> Result<(), &'static str> {
    let (_, root) = new_tmpfs();
    root.create("a.txt").map_err(|_| "create a")?;
    root.create("b.txt").map_err(|_| "create b")?;
    root.create("c.txt").map_err(|_| "create c")?;
    root.mkdir("sub").map_err(|_| "mkdir")?;
    let children = root.children().map_err(|_| "children failed")?;
    if children.len() != 4 { return Err("expected 4 children"); }
    let names: Vec<crate::alloc::string::String> = children.iter().map(|c| c.name()).collect();
    for want in &["a.txt", "b.txt", "c.txt", "sub"] {
        if !names.iter().any(|n| n == want) { return Err("missing child"); }
    }
    Ok(())
}

pub fn register() {
    crate::selftest::register("tmpfs::create_file", test_tmpfs_create_file);
    crate::selftest::register("tmpfs::create_duplicate", test_tmpfs_create_duplicate);
    crate::selftest::register("tmpfs::create_on_file", test_tmpfs_create_on_file);
    crate::selftest::register("tmpfs::create_dir_fails", test_tmpfs_create_dir_fails);
    crate::selftest::register("tmpfs::mkdir_duplicate", test_tmpfs_mkdir_duplicate);
    crate::selftest::register("tmpfs::mkdir_on_file", test_tmpfs_mkdir_on_file);
    crate::selftest::register("tmpfs::write_read", test_tmpfs_write_read);
    crate::selftest::register("tmpfs::write_append", test_tmpfs_write_append);
    crate::selftest::register("tmpfs::write_dir_fails", test_tmpfs_write_dir_fails);
    crate::selftest::register("tmpfs::read_dir_fails", test_tmpfs_read_dir_fails);
    crate::selftest::register("tmpfs::mkdir_children", test_tmpfs_mkdir_children);
    crate::selftest::register("tmpfs::nested_create", test_tmpfs_nested_create);
    crate::selftest::register("tmpfs::rename", test_tmpfs_rename);
    crate::selftest::register("tmpfs::rename_nonexistent", test_tmpfs_rename_nonexistent);
    crate::selftest::register("tmpfs::rename_to_existing", test_tmpfs_rename_to_existing);
    crate::selftest::register("tmpfs::rename_on_file", test_tmpfs_rename_on_file);
    crate::selftest::register("tmpfs::truncate", test_tmpfs_truncate);
    crate::selftest::register("tmpfs::truncate_same", test_tmpfs_truncate_same);
    crate::selftest::register("tmpfs::truncate_zero", test_tmpfs_truncate_zero);
    crate::selftest::register("tmpfs::unlink", test_tmpfs_unlink);
    crate::selftest::register("tmpfs::unlink_nonexistent", test_tmpfs_unlink_nonexistent);
    crate::selftest::register("tmpfs::unlink_on_file", test_tmpfs_unlink_on_file);
    crate::selftest::register("tmpfs::symlink_readlink", test_tmpfs_symlink_readlink);
    crate::selftest::register("tmpfs::chmod", test_tmpfs_chmod);
    crate::selftest::register("tmpfs::chown", test_tmpfs_chown);
    crate::selftest::register("tmpfs::write_overwrite", test_tmpfs_write_overwrite);
    crate::selftest::register("tmpfs::multiple_files", test_tmpfs_multiple_files);
}
