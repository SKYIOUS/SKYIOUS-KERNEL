use alloc::boxed::Box;
use smoltcp::socket::dhcpv4;
use smoltcp::wire::Ipv4Address;
use spin::Mutex;
use lazy_static::lazy_static;
use alloc::vec::Vec;

lazy_static! {
    pub static ref DHCP_DNS_SERVERS: Mutex<Vec<Ipv4Address>> = Mutex::new(Vec::new());
}

pub fn create_socket() -> dhcpv4::Socket<'static> {
    let mut socket = dhcpv4::Socket::new();
    let rx_buf: &'static mut [u8] = Box::leak(Box::new([0u8; 512]));
    socket.set_receive_packet_buffer(rx_buf);
    socket
}
