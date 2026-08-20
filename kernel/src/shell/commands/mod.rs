pub mod system;
pub mod fs;
#[cfg(feature = "net")]
pub mod net;
pub mod debug;
pub mod theme;

/// Shared command table. `out` is the output sink (GUI terminal print_str or
/// VGA println). `gui` skips console-only commands that write the VGA buffer
/// directly (clear, theme, test_pf, test_cow). Returns true if handled.
pub fn dispatch(cmd: &str, args: &[&str], out: &mut dyn FnMut(&str), gui: bool) -> bool {
    match cmd {
        "help" => { system::help(out); true }
        "info" => { system::info(out); true }
        "uptime" => { system::uptime(out); true }
        "echo" => { system::echo(args, out); true }
        "date" => { system::date(out); true }
        "whoami" => { system::whoami(out); true }
        "ps" => { system::ps(out); true }
        "mem" => { system::mem(out); true }
        "neofetch" => { system::neofetch(out); true }
        "sleep" => { system::sleep(args.first().copied().unwrap_or(""), out); true }
        "exec" => { system::exec(args.first().copied().unwrap_or(""), out); true }
        "reboot" => { system::reboot(out); true }
        "poweroff" => { system::poweroff(out); true }

        "ls" => { fs::ls(args.first().copied().unwrap_or("."), out); true }
        "cd" => { fs::cd(args.first().copied().unwrap_or(""), out); true }
        "pwd" => { fs::pwd(out); true }
        "mkdir" => { fs::mkdir(args.first().copied().unwrap_or(""), out); true }
        "rm" => { fs::rm(args.first().copied().unwrap_or(""), out); true }
        "touch" => { fs::touch(args.first().copied().unwrap_or(""), out); true }
        "cat" => { fs::cat(args.first().copied().unwrap_or(""), out); true }
        "stat" => { fs::stat(args.first().copied().unwrap_or(""), out); true }
        "cp" => { fs::cp(args.first().copied().unwrap_or(""), args.get(1).copied().unwrap_or(""), out); true }
        "mount" => { fs::mount(out); true }

        #[cfg(feature = "net")]
        "ping" => { net::ping(args.first().copied().unwrap_or(""), out); true }
        #[cfg(feature = "net")]
        "nslookup" => { net::nslookup(args.first().copied().unwrap_or(""), out); true }
        #[cfg(feature = "net")]
        "fetch" => { net::fetch(args.first().copied().unwrap_or(""), out); true }

        "heap_test" => { debug::heap_test(out); true }
        "lspci" => { debug::lspci(); true }
        "panic" => { debug::panic(); true }

        // [vga] console-only: skip from the GUI terminal
        "clear" if !gui => { crate::vga_buffer::clear_screen(); true }
        "theme" if !gui => { theme::theme(args.first().copied().unwrap_or("")); true }
        "test_pf" if !gui => { debug::test_pf(); true }
        "test_cow" if !gui => { debug::test_cow(); true }

        _ => false,
    }
}