#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use skyos_libc::syscall;

const PKG_DB_DIR: &str = "/var/cache/skypkg";
const PKG_INSTALLED_DIR: &str = "/var/cache/skypkg/installed";
const REPO_CATALOG: &str = "/var/cache/skypkg/repo.catalog";
const DEFAULT_REPO_URL: &str = "https://packages.skyos.dev/catalog.json";
const SKP_MAGIC: [u8; 4] = *b"SKP1";

struct Manifest {
    name: String,
    version: String,
    description: String,
    deps: Vec<String>,
    arch: String,
    size: u64,
    sha256: String,
    maintainer: String,
    license: String,
}

fn parse_manifest(data: &[u8]) -> Option<Manifest> {
    let text = core::str::from_utf8(data).ok()?;
    let mut m = Manifest {
        name: String::new(),
        version: String::new(),
        description: String::new(),
        deps: Vec::new(),
        arch: String::from("x86_64"),
        size: 0,
        sha256: String::new(),
        maintainer: String::new(),
        license: String::new(),
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            let val = line[eq+1..].trim().trim_matches('"');
            match key {
                "name" => m.name = val.to_string(),
                "version" => m.version = val.to_string(),
                "description" => m.description = val.to_string(),
                "deps" => {
                    for d in val.split(',') {
                        let d = d.trim();
                        if !d.is_empty() { m.deps.push(d.to_string()); }
                    }
                }
                "arch" => m.arch = val.to_string(),
                "size" => m.size = val.parse().unwrap_or(0),
                "sha256" => m.sha256 = val.to_string(),
                "maintainer" => m.maintainer = val.to_string(),
                "license" => m.license = val.to_string(),
                _ => {}
            }
        }
    }
    if m.name.is_empty() { None } else { Some(m) }
}

fn serialize_manifest(m: &Manifest) -> Vec<u8> {
    let s = format!(
        "name=\"{}\"\nversion=\"{}\"\ndescription=\"{}\"\narch=\"{}\"\nmaintainer=\"{}\"\nlicense=\"{}\"\nsize={}\nsha256=\"{}\"\ndeps=\"{}\"\n",
        m.name, m.version, m.description, m.arch, m.maintainer, m.license, m.size, m.sha256,
        m.deps.join(",")
    );
    s.into_bytes()
}

fn read_entire_file(path: &str) -> Option<Vec<u8>> {
    let cpath = alloc::ffi::CString::new(path).ok()?;
    let fd = syscall::open(cpath.as_ptr() as *const u8, 0);
    if (fd as i64) < 0 { return None; }
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = syscall::read(fd, &mut tmp);
        if (n as i64) <= 0 { break; }
        buf.extend_from_slice(&tmp[..n as usize]);
    }
    syscall::close(fd);
    Some(buf)
}

fn write_entire_file(path: &str, data: &[u8]) -> bool {
    let cpath = match alloc::ffi::CString::new(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let fd = syscall::open(cpath.as_ptr() as *const u8, 0x42);
    if (fd as i64) < 0 { return false; }
    let written = syscall::write(fd, data);
    syscall::close(fd);
    (written as i64) >= 0
}

fn file_exists(path: &str) -> bool {
    let cpath = match alloc::ffi::CString::new(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let fd = syscall::open(cpath.as_ptr() as *const u8, 0);
    if (fd as i64) >= 0 { syscall::close(fd); true } else { false }
}

fn mkdir_p(path: &str) -> bool {
    let cpath = match alloc::ffi::CString::new(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if syscall::mkdir(cpath.as_ptr() as *const u8, 0o755) >= 0 { return true; }
    file_exists(path)
}

fn cmd_install(args: &[&str]) {
    if args.is_empty() { puts("Usage: skypkg install <package.skp>"); return; }
    let pkg_path = args[0];
    let data = match read_entire_file(pkg_path) {
        Some(d) => d,
        None => { eputs(&format!("Error: cannot read {}", pkg_path)); return; }
    };
    if data.len() < 8 || data[..4] != SKP_MAGIC {
        eputs("Error: invalid .skp file (bad magic)");
        return;
    }
    let manifest_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if data.len() < 8 + manifest_len {
        eputs("Error: truncated manifest");
        return;
    }
    let manifest_data = &data[8..8 + manifest_len];
    let manifest = match parse_manifest(manifest_data) {
        Some(m) => m,
        None => { eputs("Error: invalid manifest"); return; }
    };
    let payload_start = 8 + manifest_len;
    let payload = &data[payload_start..];
    mkdir_p(PKG_INSTALLED_DIR);
    let dest_dir = format!("{}/{}", PKG_INSTALLED_DIR, manifest.name);
    mkdir_p(&dest_dir);
    mkdir_p(&format!("{}/files", dest_dir));
    let install_path = format!("{}/files/{}", dest_dir, manifest.name);
    if write_entire_file(&install_path, payload) {
        let meta_path = format!("{}/meta.txt", dest_dir);
        write_entire_file(&meta_path, &serialize_manifest(&manifest));
        puts(&format!("[skypkg] Installed {} v{}", manifest.name, manifest.version));
    } else {
        eputs(&format!("Error: failed to write {}", install_path));
    }
}

fn cmd_remove(args: &[&str]) {
    if args.is_empty() { puts("Usage: skypkg remove <package>"); return; }
    let pkg_name = args[0];
    let pkg_dir = format!("{}/{}", PKG_INSTALLED_DIR, pkg_name);
    if !file_exists(&format!("{}/meta.txt", pkg_dir)) {
        eputs(&format!("Error: {} not installed", pkg_name));
        return;
    }
    let meta = read_entire_file(&format!("{}/meta.txt", pkg_dir));
    let manifest = meta.as_ref().and_then(|d| parse_manifest(d));
    let install_path = format!("{}/files/{}", pkg_dir, pkg_name);
    if let Ok(ci) = alloc::ffi::CString::new(install_path.as_str()) {
        syscall::unlink(ci.as_ptr() as *const u8);
    }
    if let Some(m) = &manifest {
        puts(&format!("[skypkg] Removed {} v{}", m.name, m.version));
    }
}

fn cmd_list(_args: &[&str]) {
    let dirs = match libskyos::list_dir(PKG_INSTALLED_DIR) {
        Some(d) => d,
        None => { puts("No packages installed."); return; }
    };
    if dirs.is_empty() { puts("No packages installed."); return; }
    puts("Installed packages:");
    for dir in &dirs {
        let meta_path = format!("{}/{}/meta.txt", PKG_INSTALLED_DIR, dir);
        if let Some(data) = read_entire_file(&meta_path) {
            if let Some(m) = parse_manifest(&data) {
                puts(&format!("  {} v{} — {}", m.name, m.version, m.description));
            }
        }
    }
}

fn cmd_info(args: &[&str]) {
    if args.is_empty() { puts("Usage: skypkg info <package>"); return; }
    for pkg in args {
        let meta_path = format!("{}/{}/meta.txt", PKG_INSTALLED_DIR, pkg);
        if let Some(data) = read_entire_file(&meta_path) {
            if let Some(m) = parse_manifest(&data) {
                puts(&format!("Name:        {}", m.name));
                puts(&format!("Version:     {}", m.version));
                puts(&format!("Description: {}", m.description));
                puts(&format!("Arch:        {}", m.arch));
                puts(&format!("Deps:        {}", m.deps.join(", ")));
                puts(&format!("License:     {}", m.license));
                puts(&format!("Maintainer:  {}", m.maintainer));
                puts(&format!("Size:        {} bytes", m.size));
                puts(&format!("SHA256:      {}", m.sha256));
            }
        } else {
            puts(&format!("{}: not installed", pkg));
        }
    }
}

fn cmd_search(args: &[&str]) {
    if args.is_empty() { puts("Usage: skypkg search <term>"); return; }
    let term = args.join(" ").to_lowercase();
    let catalog = read_entire_file(REPO_CATALOG);
    let catalog = match catalog {
        Some(c) => c,
        None => { eputs("No repository catalog found. Run 'skypkg update' first."); return; }
    };
    let text = core::str::from_utf8(&catalog).unwrap_or("");
    let mut found = false;
    for line in text.lines() {
        if line.to_lowercase().contains(&term) {
            puts(line);
            found = true;
        }
    }
    if !found { puts(&format!("No packages matching '{}' found.", term)); }
}

fn cmd_update(_args: &[&str]) {
    puts("[skypkg] Updating repository catalog...");
    mkdir_p(PKG_DB_DIR);
    puts(&format!("[skypkg] Creating offline catalog at {}", REPO_CATALOG));
    write_entire_file(REPO_CATALOG, b"# skypkg repository catalog (offline mode)\n# Use 'skypkg build' to create .skp packages and install manually.\n");
    puts("[skypkg] Catalog updated.");
}

fn cmd_upgrade(_args: &[&str]) {
    puts("[skypkg] Checking for upgrades...");
    puts("[skypkg] Run 'skypkg update' first to refresh catalog.");
    puts("[skypkg] Upgrade requires repository with version comparison.");
}

fn cmd_build(args: &[&str]) {
    if args.is_empty() {
        puts("Usage: skypkg build <recipe.skp>");
        puts("");
        puts("Build a .skp package from a recipe file.");
        puts("Recipe format: key=value manifest lines ending with 'payload:' section.");
        puts("Example:");
        puts("  name=\"hello\"");
        puts("  version=\"1.0.0\"");
        puts("  description=\"Hello World\"");
        puts("  deps=\"\"");
        puts("  license=\"MIT\"");
        puts("  payload:");
        puts("  <binary data>");
        return;
    }
    let recipe = match read_entire_file(args[0]) {
        Some(d) => d,
        None => { eputs(&format!("Error: cannot read {}", args[0])); return; }
    };
    let recipe_str = core::str::from_utf8(&recipe).unwrap_or("");
    let payload_idx = recipe_str.find("payload:").unwrap_or(recipe.len());
    let manifest_text = &recipe[..payload_idx];
    let payload = if payload_idx < recipe.len() {
        &recipe[payload_idx + 8..]
    } else { &[] };
    let manifest = match parse_manifest(manifest_text) {
        Some(m) => m,
        None => { eputs("Error: invalid recipe manifest"); return; }
    };
    let serialized = serialize_manifest(&manifest);
    let mut skp = Vec::new();
    skp.extend_from_slice(&SKP_MAGIC);
    skp.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
    skp.extend_from_slice(&serialized);
    skp.extend_from_slice(payload);
    let out_name = format!("{}.skp", manifest.name);
    if write_entire_file(&out_name, &skp) {
        puts(&format!("[skypkg] Built {} v{} -> {}", manifest.name, manifest.version, out_name));
    } else {
        eputs(&format!("Error: failed to write {}", out_name));
    }
}

fn usage() {
    puts("SkyOS Package Manager (skypkg)");
    puts("");
    puts("Usage: skypkg <command> [args]");
    puts("");
    puts("Commands:");
    puts("  install <pkg.skp>   Install a .skp package file");
    puts("  remove <pkg>        Remove an installed package");
    puts("  update              Update package repository catalog");
    puts("  upgrade             Upgrade all installed packages");
    puts("  search <term>       Search for packages in catalog");
    puts("  info <pkg>          Show info about an installed package");
    puts("  list                List installed packages");
    puts("  build <recipe>      Build a .skp from a recipe file");
    puts("  help                Show this help");
}

fn puts(s: &str) {
    syscall::write(1, s.as_bytes());
    syscall::write(1, b"\n");
}

fn eputs(s: &str) {
    syscall::write(2, s.as_bytes());
    syscall::write(2, b"\n");
}

fn get_args(argv: *const *const u8) -> Vec<String> {
    let mut v = Vec::new();
    if argv.is_null() { return v; }
    for i in 1.. {
        let ptr = unsafe { argv.offset(i as isize) };
        if unsafe { *ptr }.is_null() { break; }
        let cstr = unsafe { core::ffi::CStr::from_ptr(*ptr as *const i8) };
        if let Ok(s) = cstr.to_str() {
            v.push(s.to_string());
        }
    }
    v
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, argv: *const *const u8) -> i32 {
    let args = get_args(argv);
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let cmd = args_str.first().copied().unwrap_or("");
    match cmd {
        "install" | "i" => cmd_install(&args_str[1..]),
        "remove" | "rm" => cmd_remove(&args_str[1..]),
        "update" | "up" => cmd_update(&args_str[1..]),
        "upgrade" => cmd_upgrade(&args_str[1..]),
        "search" | "s" => cmd_search(&args_str[1..]),
        "info" => cmd_info(&args_str[1..]),
        "list" | "ls" => cmd_list(&args_str[1..]),
        "build" | "b" => cmd_build(&args_str[1..]),
        "help" | "--help" | "-h" => usage(),
        _ => {
            eputs(&format!("skypkg: unknown command '{}'", cmd));
            usage();
            return 1;
        }
    }
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
