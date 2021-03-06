extern crate libc;
#[macro_use]
extern crate redhook;

use std::ptr;
use std::time::{SystemTime};

use libc::{c_char, c_int, size_t, sockaddr, socklen_t, ssize_t};
use rustracing::tag::Tag;
use rustracing_jaeger::reporter::JaegerCompactReporter;

use crate::singleton::{tracer, traces};
use rustracing_jaeger::span::SpanContext;
use std::collections::HashMap;
use std::sync::MutexGuard;
use std::sync::atomic::{AtomicBool, Ordering};
use httparse::{Response, Request};
use crate::inet::from_libc_sockaddr;
use nix::sys::socket::SockAddr;

mod singleton;
mod inet;

struct Trace {
    dst_addr: Option<String>,
    req_headers: Option<String>,
    req_body: Option<String>,
}

static TRACE_RUNNING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "debug")]
macro_rules! debug_print {
    ($( $args:expr ),*) => { dbg!( $( $args ),* ); }
}

// Non-debug version
#[cfg(not(feature = "debug"))]
macro_rules! debug_print {
    ($( $args:expr ),*) => {}
}

fn vec_i8_into_u8(v: Vec<i8>) -> Vec<u8> {
    let mut v = std::mem::ManuallyDrop::new(v);

    let p = v.as_mut_ptr();
    let len = v.len();
    let cap = v.capacity();

    unsafe { Vec::from_raw_parts(p as *mut u8, len, cap) }
}

fn start_trace(sockfd: c_int, payload: String, t: &mut MutexGuard<HashMap<i32, Trace>>) {
    if let Some(t) = t.get_mut(&sockfd) {
        debug_print!("PROCESS REQ");
        let mut req_headers = [httparse::EMPTY_HEADER; 16];
        let mut req = httparse::Request::new(&mut req_headers);
        let body_start = req.parse(payload.as_bytes()).unwrap().unwrap();

        let tr = tracer();
        let tr = tr.inner.try_lock().unwrap();
        let span = tr.0.span(format!("{} {}", req.method.unwrap(), req.path.unwrap()))
            .tag(Tag::new("to", format!("{}", t.dst_addr.as_ref().unwrap())))
            .start_time(SystemTime::now())
            .start();

        t.req_body = Some(payload[body_start..(payload.len()-1)].to_string());
        let headers = payload[0..body_start-2].to_string();

        let _tid = span.context().unwrap().state().to_string();

        t.req_headers = Some(format!("{}{}: {}\r\n\r\n", headers, "uber-trace-id", _tid.to_string()));
    }
}

fn end_trace(sockfd: c_int, payload: String, t: &mut MutexGuard<HashMap<i32, Trace>>) {
    if let Some(t) = t.get_mut(&sockfd) {
        if t.req_headers.is_some() {
            debug_print!("PROCESS RES");
            let mut res_headers = [httparse::EMPTY_HEADER; 16];
            let mut res = httparse::Response::new(&mut res_headers);
            res.parse(payload.as_bytes()).unwrap().unwrap();

            let req_str = t.req_headers.as_ref().unwrap();
            let mut req_headers = [httparse::EMPTY_HEADER; 16];
            let mut req = httparse::Request::new(&mut req_headers);
            req.parse(req_str.as_bytes()).unwrap();

            add_span(res, req_str, req);

            report_trace();

            TRACE_RUNNING.swap(false, Ordering::Relaxed);
        }
    }

    t.remove(&sockfd);
}

fn report_trace() {
    let tr = tracer();
    let span = &tr.inner.lock().unwrap().1;

    let reporter = JaegerCompactReporter::new("sample_service").unwrap();
    reporter.report(&span.try_iter().collect::<Vec<_>>()).unwrap();
}

fn add_span(res: Response, _req_str: &String, req: Request) {
    let tr = tracer();
    let tr = tr.inner.lock().unwrap();
    let mut carrier = HashMap::new();
    let header = req.headers;
    for field in header {
        carrier.insert(field.name, field.value);
    }

    let ctx = SpanContext::extract_from_http_header(&carrier).unwrap().unwrap();
    tr.0.span(format!("{} {}", req.method.unwrap(), req.path.unwrap()))
        .tag(Tag::new("code", format!("{}", res.code.unwrap())))
        .start_time(SystemTime::now())
        .follows_from(&ctx)
        .start();

    debug_print!(_req_str);
    debug_print!(carrier);
}

fn create_trace(sockfd: i32, t: &mut MutexGuard<HashMap<i32, Trace>>, addr_in: Option<String>) {
    let trace = Trace {
        dst_addr: addr_in,
        req_headers: None,
        req_body: None,
    };
    t.insert(sockfd, trace);
}

fn add_trace(sockfd: c_int, addr_in: Option<SockAddr>, t: &mut MutexGuard<HashMap<i32, Trace>>) {
    if !t.contains_key(&sockfd) && addr_in.is_none() {
        debug_print!("ADDTRACE w/o addr");
        create_trace(sockfd, t, None);
    }

    if let Some(addr_in) = addr_in {
        if t.contains_key(&sockfd) {
            debug_print!("ADDTRACE w/ addr");
            create_trace(sockfd, t, Some(addr_in.to_str()));
        }
    }
}

hook! {
    unsafe fn socket(domain: c_int, socktype: c_int, protocol: c_int) -> c_int => my_socket {
        let sockfd = real!(socket)(domain, socktype, protocol);
        debug_print!("SOCKET", sockfd, domain, socktype, protocol);
        if domain == 2 {
            let running = TRACE_RUNNING.swap(true, Ordering::Relaxed);
            if !running {
                let t = traces();
                let mut t = t.inner.try_lock().unwrap();
                add_trace(sockfd, None, &mut t);
            }
        }
        sockfd
    }
}

hook! {
    unsafe fn connect(sockfd: c_int, sockaddr_ptr: *mut sockaddr, len: socklen_t) -> isize => my_connect {
        let retval = real!(connect)(sockfd, sockaddr_ptr, len);
        let addr_in = from_libc_sockaddr(sockaddr_ptr);
        if let Some(_a) = addr_in {
            debug_print!("CONNECT", sockfd, _a.to_str());
            let t = traces();
            let mut t = t.inner.try_lock().unwrap();
            add_trace(sockfd, addr_in, &mut t);
        }
        retval
    }
}

hook! {
    unsafe fn recv(sockfd: c_int, _ptr: *mut c_char, len: size_t, flags: c_int) -> ssize_t => my_recv {
        debug_print!("RECV");
        let retval = real!(recv)(sockfd, _ptr, len, flags);
        let mut vec: Vec<i8> = vec![0; 8192];
        ptr::copy_nonoverlapping(_ptr as *mut i8, vec.as_mut_ptr(), vec.len());
        let vec2: Vec<u8> = vec_i8_into_u8(vec);
        let payload: String = String::from_utf8_lossy(&vec2).to_string();
        let t = traces();
        let mut t = t.inner.try_lock().unwrap();
        end_trace(sockfd, payload, &mut t);
        retval
    }
}

hook! {
    unsafe fn send(sockfd: c_int, _ptr: *mut c_char, len: size_t, flags: c_int) -> ssize_t => my_send {
        debug_print!("SEND");
        let retval = real!(send)(sockfd, _ptr, len, flags);
        let mut vec: Vec<i8> = vec![0; 8192];
        ptr::copy_nonoverlapping(_ptr as *mut i8, vec.as_mut_ptr(), vec.len());
        let vec2: Vec<u8> = vec_i8_into_u8(vec);
        let payload: String = String::from_utf8_lossy(&vec2).to_string();
        let t = traces();
        let mut t = t.inner.try_lock().unwrap();
        start_trace(sockfd, payload, &mut t);
        retval

    }
}

hook! {
    unsafe fn read(sockfd: c_int, _ptr: *mut libc::c_void, len: size_t) -> ssize_t => my_read {
        let retval = real!(read)(sockfd, _ptr, len);
        let t = traces();
        let mut t = t.inner.try_lock().unwrap();
        if t.contains_key(&sockfd) {
            debug_print!("READ", sockfd, t.get(&sockfd).unwrap().dst_addr.as_ref().unwrap());
            let mut vec: Vec<i8> = vec![0; 8192];
            ptr::copy_nonoverlapping(_ptr as *mut i8, vec.as_mut_ptr(), vec.len());
            let vec2: Vec<u8> = vec_i8_into_u8(vec);
            let payload: String = String::from_utf8_lossy(&vec2).to_string();
            end_trace(sockfd, payload, &mut t);
        }
        retval
    }
}

hook! {
    unsafe fn write(sockfd: c_int, _ptr: *mut libc::c_void, len: size_t) -> ssize_t => my_write {
        let mut vec: Vec<i8> = vec![0; 8192];
        ptr::copy_nonoverlapping(_ptr as *mut i8, vec.as_mut_ptr(), vec.len());
        let vec2: Vec<u8> = vec_i8_into_u8(vec);
        let payload: String = String::from_utf8_lossy(&vec2).to_string();
        let retval = real!(write)(sockfd, _ptr, len);
        if payload.starts_with("GET") {
            let t = traces();
            let mut t = t.inner.try_lock().unwrap();
            if t.contains_key(&sockfd) {
                debug_print!("WRITE");
                start_trace(sockfd, payload, &mut t);
            }
        }
        retval
    }
}
