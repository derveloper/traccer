extern crate libc;

#[macro_use]
extern crate redhook;

#[macro_use]
extern crate lazy_static;

use libc::{size_t, ssize_t, c_int, c_char, socklen_t, sockaddr};
use std::{ptr};
use std::collections::HashMap;
use std::sync::Mutex;
use nix::sys::socket::{AddressFamily, VsockAddr, SockAddr, LinkAddr, NetlinkAddr, InetAddr};

struct Trace {
    dst_addr: String,
    req: Option<String>,
    res: Option<String>,
}

lazy_static! {
    static ref TRACES: Mutex<HashMap<i32, Trace>> = Mutex::new(HashMap::new());
}

//noinspection RsBorrowChecker
unsafe fn from_libc_sockaddr(addr: *const sockaddr) -> Option<SockAddr> {
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

fn vec_i8_into_u8(v: Vec<i8>) -> Vec<u8> {
    let mut v = std::mem::ManuallyDrop::new(v);

    let p = v.as_mut_ptr();
    let len = v.len();
    let cap = v.capacity();

    unsafe { Vec::from_raw_parts(p as *mut u8, len, cap) }
}

fn process_response(sockfd: c_int, payload: String) {
    {
        let mut t = TRACES.lock().unwrap();
        if let Some(t) = t.get_mut(&sockfd) {
            t.res = Some(payload);
            if t.req.is_some() {
                let req_str = t.req.as_ref().unwrap();
                let res_str = t.res.as_ref().unwrap();

                let mut req_headers = [httparse::EMPTY_HEADER; 16];
                let mut req = httparse::Request::new(&mut req_headers);
                req.parse(req_str.as_bytes()).unwrap();

                let mut res_headers = [httparse::EMPTY_HEADER; 16];
                let mut res = httparse::Response::new(&mut res_headers);
                res.parse(res_str.as_bytes()).unwrap();

                println!("addr: {} -> {} {} status: {}", t.dst_addr, req.method.unwrap(), req.path.unwrap(), res.code.unwrap());
            }
        }
    }
    TRACES.lock().unwrap().remove(&sockfd);
}

fn process_request(sockfd: c_int, payload: String) {
    {
        let mut t = TRACES.lock().unwrap();
        if let Some(t) = t.get_mut(&sockfd) {
            t.req = Some(payload);
        }
    }
}

fn add_trace(sockfd: c_int, addr_in: Option<SockAddr>) {
    if let Some(addr_in) = addr_in {
        let trace = Trace {
            dst_addr: addr_in.to_str(),
            req: None,
            res: None
        };
        if !TRACES.lock().unwrap().contains_key(&sockfd) {
            TRACES.lock().unwrap().insert(sockfd, trace);
        }
    }
}

hook! {
    unsafe fn connect(sockfd: c_int, sockaddr_ptr: *mut sockaddr, len: socklen_t) -> isize => my_connect {
        let retval = real!(connect)(sockfd, sockaddr_ptr, len);
        let addr_in = from_libc_sockaddr(sockaddr_ptr);
        add_trace(sockfd, addr_in);
        retval
    }
}

hook! {
    unsafe fn recv(sockfd: c_int, _ptr: *mut c_char, len: size_t, flags: c_int) -> ssize_t => my_recv {
        let retval = real!(recv)(sockfd, _ptr, len, flags);
        let mut vec: Vec<i8> = vec![0; 8192];
        ptr::copy_nonoverlapping(_ptr as *mut i8, vec.as_mut_ptr(), vec.len());
        let vec2: Vec<u8> = vec_i8_into_u8(vec);
        let payload: String = String::from_utf8_lossy(&vec2).to_string();
        process_response(sockfd, payload);
        retval
    }
}

hook! {
    unsafe fn send(sockfd: c_int, _ptr: *mut c_char, len: size_t, flags: c_int) -> ssize_t => my_send {
        let retval = real!(send)(sockfd, _ptr, len, flags);
        let mut vec: Vec<i8> = vec![0; 8192];
        ptr::copy_nonoverlapping(_ptr as *mut i8, vec.as_mut_ptr(), vec.len());
        let vec2: Vec<u8> = vec_i8_into_u8(vec);
        let payload: String = String::from_utf8_lossy(&vec2).to_string();
        process_request(sockfd, payload);
        retval
    }
}
