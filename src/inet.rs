extern crate libc;
use nix::sys::socket::{AddressFamily, SockAddr, InetAddr, VsockAddr, NetlinkAddr, LinkAddr};
use libc::{sockaddr};

// this is taken from nix rust bindings: https://github.com/nix-rust/nix
// all credits goes to them. license: MIT
//noinspection RsBorrowChecker
pub(crate) unsafe fn from_libc_sockaddr(addr: *const sockaddr) -> Option<SockAddr> {
    if addr.is_null() {
        None
    } else {
        match AddressFamily::from_i32(i32::from((*addr).sa_family)) {
            Some(AddressFamily::Unix) => None,
            Some(AddressFamily::Inet) => Some(SockAddr::Inet(
                InetAddr::V4(*(addr as *const libc::sockaddr_in)))),
            Some(AddressFamily::Inet6) => Some(SockAddr::Inet(
                InetAddr::V6(*(addr as *const libc::sockaddr_in6)))),
            #[cfg(any(target_os = "android", target_os = "linux"))]
            Some(AddressFamily::Netlink) => Some(SockAddr::Netlink(
                NetlinkAddr(*(addr as *const libc::sockaddr_nl)))),
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            Some(AddressFamily::System) => Some(SockAddr::SysControl(
                SysControlAddr(*(addr as *const libc::sockaddr_ctl)))),
            #[cfg(any(target_os = "android", target_os = "linux"))]
            Some(AddressFamily::Packet) => Some(SockAddr::Link(
                LinkAddr(*(addr as *const libc::sockaddr_ll)))),
            #[cfg(any(target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "ios",
            target_os = "macos",
            target_os = "netbsd",
            target_os = "illumos",
            target_os = "openbsd"))]
            Some(AddressFamily::Link) => {
                let ether_addr = LinkAddr(*(addr as *const libc::sockaddr_dl));
                if ether_addr.is_empty() {
                    None
                } else {
                    Some(SockAddr::Link(ether_addr))
                }
            }
            #[cfg(any(target_os = "android", target_os = "linux"))]
            Some(AddressFamily::Vsock) => Some(SockAddr::Vsock(
                VsockAddr(*(addr as *const libc::sockaddr_vm)))),
            // Other address families are currently not supported and simply yield a None
            // entry instead of a proper conversion to a `SockAddr`.
            Some(_) | None => None,
        }
    }
}
