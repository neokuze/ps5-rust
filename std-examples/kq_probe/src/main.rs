//! Diagnostic: runs the kqueue/fd calls tokio needs, plus tokio runtime
//! builds, and serves the results on port 9957 (curl http://<ps5-ip>:9957).
//! This is how the kevent and F_DUPFD_CLOEXEC fixes in ps5-std-rt were found.
use ps5::Toast;
use ps5_std_rt::Kevent12;
use std::ffi::c_int;
use std::io::Error;

extern "C" {
    fn kqueue() -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn pipe2(fds: *mut c_int, flags: c_int) -> c_int;
    fn kevent(kq: c_int, ch: *const Kevent12, nch: c_int, ev: *mut Kevent12, nev: c_int, t: *const [i64; 2]) -> c_int;
}

const EVFILT_READ: i16 = -1;
const EVFILT_USER: i16 = -11;
const EV_ADD: u16 = 0x1;
const EV_CLEAR: u16 = 0x20;
const EV_RECEIPT: u16 = 0x40;

fn r(ret: c_int) -> String {
    if ret < 0 { format!("ERR {}", Error::last_os_error().raw_os_error().unwrap_or(0)) } else { format!("ok {ret}") }
}

fn ev(fd: usize, filter: i16, flags: u16) -> Kevent12 {
    Kevent12 { ident: fd, filter, flags, fflags: 0, data: 0, udata: std::ptr::null_mut(), ext: [0; 4] }
}

/// Results are too long for toasts: serve them on port 9957 instead, to
/// one client (curl http://<ps5-ip>:9957), then exit.
fn serve(lines: &[String]) {
    use std::io::{Read, Write};
    let listener = match std::net::TcpListener::bind("0.0.0.0:9957") {
        Ok(l) => l,
        Err(e) => {
            let _ = Toast::new(&format!("bind {e}")).sub("kq_probe").send();
            return;
        }
    };
    let _ = Toast::new("results on port 9957").sub("kq_probe").send();
    if let Ok((mut s, _)) = listener.accept() {
        let mut req = [0u8; 1024];
        let _ = s.read(&mut req);
        let body = lines.join("\n") + "\n";
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = Toast::new("results sent").sub("kq_probe").send();
    }
}

fn main() {
    let _ = Toast::new("started").sub("kq_probe").send();
    let mut all = Vec::new();
    let mut out = Vec::new();
    unsafe {
        let kq = kqueue();
        out.push(format!("kqueue {}", r(kq)));
        out.push(format!("cloexec {}", r(fcntl(kq, 2, 1))));

        let mut p = [0 as c_int; 2];
        out.push(format!("pipe {}", r(pipe(p.as_mut_ptr()))));
        let mut p2 = [0 as c_int; 2];
        out.push(format!("pipe2 {}", r(pipe2(p2.as_mut_ptr(), 0x100000 | 0x4))));

        let zero = [0i64; 2];
        let mut res = [ev(0, 0, 0); 1];
        let c = ev(p[0] as usize, EVFILT_READ, EV_ADD | EV_CLEAR);
        out.push(format!("read {}", r(kevent(kq, &c, 1, std::ptr::null_mut(), 0, &zero))));
        let c = ev(p[0] as usize, EVFILT_READ, EV_ADD | EV_CLEAR | EV_RECEIPT);
        let n = kevent(kq, &c, 1, res.as_mut_ptr(), 1, &zero);
        out.push(format!("receipt {} data {}", r(n), res[0].data));
        let c = ev(7, EVFILT_USER, EV_ADD | EV_CLEAR);
        out.push(format!("user {}", r(kevent(kq, &c, 1, std::ptr::null_mut(), 0, &zero))));
        out.push(format!("wait {}", r(kevent(kq, std::ptr::null(), 0, res.as_mut_ptr(), 1, &zero))));

        // tokio duplicates the kqueue fd (registry().try_clone()).
        extern "C" { fn dup(fd: c_int) -> c_int; }
        out.push(format!("dup(kq) {}", r(dup(kq))));
        out.push(format!("F_DUPFD(kq) {}", r(fcntl(kq, 0, 3))));
        out.push(format!("F_DUPFD_CLOEXEC(kq) {}", r(fcntl(kq, 17, 3))));
        out.push(format!("F_DUPFD_CLOEXEC(pipe) {}", r(fcntl(p[0], 17, 3))));
        out.push(format!("std try_clone(pipe) {:?}", {
            use std::os::fd::BorrowedFd;
            BorrowedFd::borrow_raw(p[1]).try_clone_to_owned().map(|_| ()).map_err(|e| e.raw_os_error())
        }));
        out.push(format!("std try_clone(kq) {:?}", {
            use std::os::fd::BorrowedFd;
            BorrowedFd::borrow_raw(kq).try_clone_to_owned().map(|_| ()).map_err(|e| e.raw_os_error())
        }));    }
    all.extend(out);
    let _ = Toast::new("part 1 done").sub("kq_probe").send();

    let mut out = Vec::new();
    let t = |b: &mut tokio::runtime::Builder| b.build().map(|_| "ok".to_string()).unwrap_or_else(|e| format!("ERR {e}"));
    out.push(format!("ct {}", t(&mut tokio::runtime::Builder::new_current_thread())));
    out.push(format!("ct+io {}", t(tokio::runtime::Builder::new_current_thread().enable_io())));
    out.push(format!("ct+time {}", t(tokio::runtime::Builder::new_current_thread().enable_time())));
    out.push(format!("mt {}", t(&mut tokio::runtime::Builder::new_multi_thread())));
    out.push(format!("cpus {:?}", std::thread::available_parallelism().map(|n| n.get()).map_err(|e| e.raw_os_error())));
    all.extend(out);
    serve(&all);
}
