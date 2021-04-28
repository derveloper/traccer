extern crate libc;
#[macro_use]
extern crate redhook;

use std::ptr;
use std::time::{SystemTime};

use libc::{c_char, c_int, size_t, sockaddr, socklen_t, ssize_t};
use nix::sys::socket::{AddressFamily, InetAddr, LinkAddr, NetlinkAddr, SockAddr, VsockAddr};
use rustracing::tag::Tag;
use rustracing_jaeger::reporter::JaegerCompactReporter;

use crate::singleton::{tracer, traces};
use rustracing_jaeger::span::SpanContext;
use std::collections::HashMap;

mod singleton;

struct Trace {
    dst_addr: Box<String>,
    req_headers: Option<String>,
    req_body: Option<String>,
}

// this is taken from nix rust bindings: https://github.com/nix-rust/nix
// all credits goes to them. license: MIT
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

fn process_request(sockfd: c_int, payload: String) {
    let t = traces();
    let mut t = t.inner.lock().unwrap();
    if let Some(t) = t.get_mut(&sockfd) {
        println!("PROCESS REQ");
        let mut req_headers = [httparse::EMPTY_HEADER; 16];
        let mut req = httparse::Request::new(&mut req_headers);
        let body_start = req.parse(payload.as_bytes()).unwrap().unwrap();

        let tr = tracer();
        let tr = tr.inner.lock().unwrap();
        let span = tr.0.span(format!("{} {}", req.method.unwrap(), req.path.unwrap()))
            .tag(Tag::new("to", format!("{}", t.dst_addr)))
            .start_time(SystemTime::now())
            .start();

        t.req_body = Some(payload[body_start..(payload.len()-1)].to_string());
        let headers = payload[0..body_start-2].to_string();

        let _tid = span.context().unwrap().state().to_string();

        t.req_headers = Some(format!("{}{}: {}\r\n\r\n", headers, "uber-trace-id", _tid.to_string()));
    }
}

fn process_response(sockfd: c_int, payload: String) {
    {
        let t = traces();
        let mut t = t.inner.lock().unwrap();
        if let Some(t) = t.get_mut(&sockfd) {
            if t.req_headers.is_some() {
                println!("PROCESS RES");
                let mut res_headers = [httparse::EMPTY_HEADER; 16];
                let mut res = httparse::Response::new(&mut res_headers);
                res.parse(payload.as_bytes()).unwrap().unwrap();

                let req_str = t.req_headers.as_ref().unwrap();

                let mut req_headers = [httparse::EMPTY_HEADER; 16];
                let mut req = httparse::Request::new(&mut req_headers);
                req.parse(req_str.as_bytes()).unwrap();

                {
                    let tr = tracer();
                    let tr = tr.inner.lock().unwrap();
                    let mut carrier = HashMap::new();
                    let header = req.headers;
                    for field in header {
                        carrier.insert(field.name, field.value);
                    }
                    println!("{:?}", req_str);
                    println!("{:?}", carrier);
                    let ctx = SpanContext::extract_from_http_header(&carrier).unwrap().unwrap();
                    tr.0.span(format!("{} {}", req.method.unwrap(), req.path.unwrap()))
                        .tag(Tag::new("code", format!("{}", res.code.unwrap())))
                        .start_time(SystemTime::now())
                        .follows_from(&ctx)
                        .start();
                }

                let tr = tracer();
                let span = &tr.inner.lock().unwrap().1;

                let reporter = JaegerCompactReporter::new("sample_service").unwrap();
                reporter.report(&span.try_iter().collect::<Vec<_>>()).unwrap();
            }
        }
    }
    traces().inner.lock().unwrap().remove(&sockfd);
}

fn add_trace(sockfd: c_int, addr_in: Option<SockAddr>) {
    if let Some(addr_in) = addr_in {
        let trace = Trace {
            dst_addr: Box::new(addr_in.to_str()),
            req_headers: None,
            req_body: None,
        };
        if !traces().inner.lock().unwrap().contains_key(&sockfd) {
            println!("ADDTRACE");
            traces().inner.lock().unwrap().insert(sockfd, trace);
        }
    }
}

hook! {
    unsafe fn connect(sockfd: c_int, sockaddr_ptr: *mut sockaddr, len: socklen_t) -> isize => my_connect {
        println!("CONNECT");
        let retval = real!(connect)(sockfd, sockaddr_ptr, len);
        let addr_in = from_libc_sockaddr(sockaddr_ptr);
        add_trace(sockfd, addr_in);
        retval
    }
}

hook! {
    unsafe fn recv(sockfd: c_int, _ptr: *mut c_char, len: size_t, flags: c_int) -> ssize_t => my_recv {
        println!("RECV");
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
        println!("SEND");
        let retval = real!(send)(sockfd, _ptr, len, flags);
        let mut vec: Vec<i8> = vec![0; 8192];
        ptr::copy_nonoverlapping(_ptr as *mut i8, vec.as_mut_ptr(), vec.len());
        let vec2: Vec<u8> = vec_i8_into_u8(vec);
        let payload: String = String::from_utf8_lossy(&vec2).to_string();
        process_request(sockfd, payload);
        retval
    }
}
