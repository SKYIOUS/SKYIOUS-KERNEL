use alloc::sync::Arc;
use crate::objects::{KernelObject, ObjectHeader, ObjectTypeId, security::SecurityDescriptor};
use smoltcp::iface::SocketHandle;

/// Canonical SocketObject: wraps a smoltcp SocketHandle as a KernelObject.
///
/// Lives in `objects/` because it belongs to the object system. Uses
/// `task::process::SocketType` as the single enum (includes Unix variant).
pub struct SocketObject {
    pub header: ObjectHeader,
    pub handle: SocketHandle,
    pub socket_type: crate::task::process::SocketType,
}

impl SocketObject {
    pub fn new(handle: SocketHandle, socket_type: crate::task::process::SocketType) -> Arc<Self> {
        let sec = SecurityDescriptor::default_socket();
        let header = ObjectHeader::new(ObjectTypeId(6), sec);
        let name = match socket_type {
            crate::task::process::SocketType::Tcp => "TcpSocket",
            crate::task::process::SocketType::Udp => "UdpSocket",
            crate::task::process::SocketType::Raw => "RawSocket",
            crate::task::process::SocketType::Unix => "UnixSocket",
        };
        *header.name.lock() = Some(alloc::format!("Socket/{}", name));
        Arc::new(SocketObject { header, handle, socket_type })
    }
}

impl KernelObject for SocketObject {
    fn header(&self) -> &ObjectHeader { &self.header }

    fn type_name(&self) -> &'static str {
        match self.socket_type {
            crate::task::process::SocketType::Tcp => "TcpSocket",
            crate::task::process::SocketType::Udp => "UdpSocket",
            crate::task::process::SocketType::Raw => "RawSocket",
            crate::task::process::SocketType::Unix => "UnixSocket",
        }
    }

    fn query_name(&self) -> Option<alloc::string::String> {
        self.header.name.lock().clone()
    }

    fn poll_readable(&self) -> bool {
        let sockets = crate::net::SOCKETS.lock();
        for (h, socket) in sockets.iter() {
            if h == self.handle {
                use smoltcp::socket::Socket;
                if let Socket::Tcp(ref tcp) = socket { return tcp.may_recv(); }
            }
        }
        false
    }

    fn poll_writable(&self) -> bool {
        let sockets = crate::net::SOCKETS.lock();
        for (h, socket) in sockets.iter() {
            if h == self.handle {
                use smoltcp::socket::Socket;
                if let Socket::Tcp(ref tcp) = socket { return tcp.may_send(); }
            }
        }
        true
    }
}

/// Helper: create a SocketObject and register it in the namespace.
pub fn register_socket(
    handle: SocketHandle,
    stype: crate::task::process::SocketType,
    name: &str,
) -> Arc<SocketObject> {
    let obj = SocketObject::new(handle, stype);
    let path = alloc::format!("System/Sockets/{}", name);
    crate::objects::namespace::OBJECT_NAMESPACE.lock().insert(&path, obj.clone());
    obj
}
