use crate::ash::{HookPoint, AshResult};
use crate::ash::runtime::execute_handler;
use crate::ash::manager;

/// Context passed to network ASH handlers.
#[repr(C)]
struct NetContext {
    interface: u8,
    protocol: u8,
    src_port: u16,
    dst_port: u16,
    _pad: [u8; 26],
}

/// Hook into network receive path.
pub fn hook_net_receive(
    packet: &mut [u8],
    interface: u8,
    protocol: u8,
    src_port: u16,
    dst_port: u16,
) -> AshResult {
    let hook = HookPoint::NetReceive {
        interface,
        port: dst_port,
        protocol: crate::ash::Protocol::from_u8(protocol).unwrap_or(crate::ash::Protocol::Raw),
    };

    let ids = manager::lookup_ids(&hook);
    if ids.is_empty() {
        return AshResult::Continue;
    }

    let ctx = NetContext {
        interface,
        protocol,
        src_port,
        dst_port,
        _pad: [0u8; 26],
    };
    let ctx_bytes = unsafe {
        core::slice::from_raw_parts(&ctx as *const _ as *const u8, core::mem::size_of::<NetContext>())
    };

    let mut result = AshResult::Continue;
    for id in ids {
        if let Some(handler) = manager::get_verified(id) {
            result = execute_handler(&handler, ctx_bytes, packet);
            match &result {
                AshResult::Handled | AshResult::Drop => break,
                _ => {}
            }
        }
    }
    result
}

/// Hook into network transmit path.
#[allow(dead_code)]
pub fn hook_net_transmit(
    packet: &mut [u8],
    interface: u8,
    protocol: u8,
    src_port: u16,
    dst_port: u16,
) -> AshResult {
    let hook = HookPoint::NetTransmit {
        interface,
        port: dst_port,
        protocol: crate::ash::Protocol::from_u8(protocol).unwrap_or(crate::ash::Protocol::Raw),
    };

    let ids = manager::lookup_ids(&hook);
    if ids.is_empty() {
        return AshResult::Continue;
    }

    let ctx = NetContext {
        interface,
        protocol,
        src_port,
        dst_port,
        _pad: [0u8; 26],
    };
    let ctx_bytes = unsafe {
        core::slice::from_raw_parts(&ctx as *const _ as *const u8, core::mem::size_of::<NetContext>())
    };

    let mut result = AshResult::Continue;
    for id in ids {
        if let Some(handler) = manager::get_verified(id) {
            result = execute_handler(&handler, ctx_bytes, packet);
            match &result {
                AshResult::Handled | AshResult::Drop => break,
                _ => {}
            }
        }
    }
    result
}
