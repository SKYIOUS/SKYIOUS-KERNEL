//! Pins the allocation-free panic path (I7).
//!
//! `handle_panic` itself cannot be exercised in-harness: it ends in a
//! `hlt` loop, so calling it would freeze the boot instead of returning a
//! TAP result. What these tests pin instead is the exact formatting
//! primitive and shapes the panic handler and stack walker rely on:
//!
//! - `IrqFmtBuf` (`interrupts/diag.rs`) is a stack-buffer `core::fmt::
//!   Write`. `serial_fmt` is literally `IrqFmtBuf` → `serial_write`, so
//!   the buffer behavior here is the buffer behavior of every panic line.
//!   The path allocates nothing: no `Vec`/`String`/`format!` exists
//!   between the panic message and the UART (regression = allocation
//!   failure panic recurses to a banner-only 40k-loop, observed).
//! - The `format_args!` shapes below are copied verbatim from
//!   `panic_handler.rs` (register dump) and `debug/symbols.rs`
//!   (backtrace walker); a change to either panics-side shape or this
//!   test is a deliberate change to the panic output contract.

use crate::interrupts::IrqFmtBuf;

/// The bare formatting primitive must render values into a stack buffer
/// with no allocation and exact bytes (this is serial_fmt's innards).
fn test_irqfmtbuf_formats() -> Result<(), &'static str> {
    let mut buf = [0u8; 64];
    let len;
    {
        let mut w = IrqFmtBuf {
            buf: &mut buf,
            len: 0,
        };
        core::fmt::write(&mut w, format_args!("pid={} hex={:#x}", 42u64, 0xdead_u64))
            .map_err(|_| "fmt write failed")?;
        len = w.len;
    }
    let s = core::str::from_utf8(&buf[..len]).map_err(|_| "not utf8")?;
    if s != "pid=42 hex=0xdead" {
        return Err("IrqFmtBuf rendered wrong bytes");
    }
    Ok(())
}

/// Over-long input truncates at capacity (never overflows, never
/// panics) — the panic path must survive a huge message.
fn test_irqfmtbuf_truncates() -> Result<(), &'static str> {
    let mut buf = [0u8; 8];
    let len;
    {
        let mut w = IrqFmtBuf {
            buf: &mut buf,
            len: 0,
        };
        core::fmt::write(&mut w, format_args!("1234567890")).map_err(|_| "fmt write failed")?;
        len = w.len;
    }
    if len != 8 {
        return Err("truncation length wrong");
    }
    let s = core::str::from_utf8(&buf[..len]).map_err(|_| "not utf8")?;
    if s != "12345678" {
        return Err("truncated content wrong");
    }
    Ok(())
}

/// The register-dump line shape from `panic_handler.rs` must keep
/// formatting through the stack buffer exactly as serial_fmt emits it.
fn test_register_line_shape() -> Result<(), &'static str> {
    let mut buf = [0u8; 128];
    let len;
    {
        let mut w = IrqFmtBuf {
            buf: &mut buf,
            len: 0,
        };
        core::fmt::write(
            &mut w,
            format_args!("  RAX={:016x} RBX={:016x}\n", 0x1234_u64, 0xabcdef_u64),
        )
        .map_err(|_| "fmt write failed")?;
        len = w.len;
    }
    let s = core::str::from_utf8(&buf[..len]).map_err(|_| "not utf8")?;
    if s != "  RAX=0000000000001234 RBX=0000000000abcdef\n" {
        return Err("register line shape changed");
    }
    Ok(())
}

/// The backtrace-walker line shape from `debug/symbols.rs` (one symbol
/// per line through serial_fmt) must keep formatting unchanged.
fn test_backtrace_line_shape() -> Result<(), &'static str> {
    let mut buf = [0u8; 128];
    let len;
    {
        let mut w = IrqFmtBuf {
            buf: &mut buf,
            len: 0,
        };
        core::fmt::write(
            &mut w,
            format_args!(
                "  [{:016x}] {}\n",
                0xffffffff8003584d_u64, "<unknown symbol>"
            ),
        )
        .map_err(|_| "fmt write failed")?;
        len = w.len;
    }
    let s = core::str::from_utf8(&buf[..len]).map_err(|_| "not utf8")?;
    if s != "  [ffffffff8003584d] <unknown symbol>\n" {
        return Err("backtrace line shape changed");
    }
    Ok(())
}

pub fn register() {
    crate::selftest::register("panic:irqfmtbuf_formats", test_irqfmtbuf_formats);
    crate::selftest::register("panic:irqfmtbuf_truncates", test_irqfmtbuf_truncates);
    crate::selftest::register("panic:register_line_shape", test_register_line_shape);
    crate::selftest::register("panic:backtrace_line_shape", test_backtrace_line_shape);
}
