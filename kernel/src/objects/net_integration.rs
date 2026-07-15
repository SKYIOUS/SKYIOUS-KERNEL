use alloc::sync::Arc;
use crate::objects::{KernelObject, ObjectHeader, TYPE_SOCKET, security::SecurityDescriptor};
use smoltcp::iface::SocketHandle;

/// Wraps a smoltcp socket handle as a KernelObject.
pub struct SocketObject {
    pub header: ObjectHeader,
    pub handle: SocketHandle,
    pub socket_type: SocketType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SocketType {
    Tcp,
    Udp,
    Raw,
}

impl SocketObject {
    pub fn new(handle: SocketHandle, stype: SocketType) -> Arc<Self> {
        let sec = SecurityDescriptor::new(0, 0, 0o666);
        let header = ObjectHeader::new(TYPE_SOCKET, sec);
        *header.name.lock() = Some(alloc::format!("Socket/{:?}", stype));
        Arc::new(SocketObject { header, handle, socket_type: stype })
    }
}

impl KernelObject for SocketObject {
    fn header(&self) -> &ObjectHeader { &self.header }
    fn type_name(&self) -> &'static str {
        match self.socket_type {
            SocketType::Tcp => "TcpSocket",
            SocketType::Udp => "UdpSocket",
            SocketType::Raw => "RawSocket",
        }
    }
    fn query_name(&self) -> Option<alloc::string::String> {
        self.header.name.lock().clone()
    }
}

/// Helper: create a SocketObject and register it in the namespace.
pub fn register_socket(handle: SocketHandle, stype: SocketType, name: &str) -> Arc<SocketObject> {
    let obj = SocketObject::new(handle, stype);
    let path = alloc::format!("System/Sockets/{}", name);
    crate::objects::namespace::OBJECT_NAMESPACE.lock().insert(&path, obj.clone());
    obj
}
