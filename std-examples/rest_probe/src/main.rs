//! Diagnostic for rest mode: does a payload survive suspend, and what does
//! it see when the console wakes?
//!
//! A thread samples CLOCK_REALTIME and CLOCK_MONOTONIC every second and logs
//! every gap over 10 s (the same signal ps5upload's wake watchdog uses: it
//! gets no event from the system), with the uid/euid before and after. The
//! main thread serves the log on port 10001; if accept starts failing after
//! a wake it rebinds the port and logs that too. Writes nothing to disk and
//! makes no kernel writes. `curl http://<ps5-ip>:10001/quit` exits.
use ps5::Toast;
use ps5_std_rt as _;
use std::ffi::c_int;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PORT: u16 = 10001;
const GAP_S: i64 = 10;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(clock: c_int, ts: *mut Timespec) -> c_int;
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn syscall(n: i64, ...) -> c_int;
}

fn now(clock: c_int) -> f64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(clock, &mut ts) };
    ts.sec as f64 + ts.nsec as f64 / 1e9
}
fn realtime() -> f64 { now(0) }
fn monotonic() -> f64 { now(4) }
fn ids() -> String {
    unsafe { format!("uid {} euid {}", getuid(), geteuid()) }
}

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("rest_probe").send();
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!(" @ {}:{}", l.file(), l.line())).unwrap_or_default();
        toast(&format!("panic: {msg}{at}"));
    }));
    toast("started");
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let (real0, mono0) = (realtime(), monotonic());
    log.lock().unwrap().push(format!("start: realtime {real0:.0}, monotonic {mono0:.1}, {}", ids()));
    // std::thread::sleep goes through clock_nanosleep (244): see how the
    // kernel answers it directly, then that the ps5-std-rt shim sleeps.
    let ts = Timespec { sec: 0, nsec: 1_000_000 };
    let r = unsafe { syscall(244, 4, 0, &ts as *const Timespec, std::ptr::null_mut::<Timespec>()) };
    let e = std::io::Error::last_os_error();
    log.lock().unwrap().push(format!("raw clock_nanosleep syscall: {r}{}", if r != 0 { format!(" ({e})") } else { String::new() }));
    let t = monotonic();
    std::thread::sleep(Duration::from_millis(200));
    log.lock().unwrap().push(format!("thread::sleep(200ms) took {:.3}s", monotonic() - t));

    let l = log.clone();
    std::thread::Builder::new().name("rest-watch".into()).spawn(move || {
        let (mut real, mut mono) = (realtime(), monotonic());
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let (r, m) = (realtime(), monotonic());
            let (dr, dm) = (r - real, m - mono);
            if dr > GAP_S as f64 || dm > GAP_S as f64 {
                let line = format!(
                    "GAP at realtime {r:.0}: realtime +{dr:.1}s, monotonic +{dm:.1}s (slept 1s), {}",
                    ids()
                );
                toast(&format!("woke? real +{dr:.0}s mono +{dm:.0}s"));
                l.lock().unwrap().push(line);
            }
            (real, mono) = (r, m);
        }
    }).expect("spawn");

    toast(&format!("watching; log on port {PORT}"));
    let mut listener = bind();
    loop {
        let Some(lst) = &listener else {
            std::thread::sleep(Duration::from_secs(2));
            listener = bind();
            if listener.is_some() {
                log.lock().unwrap().push(format!("rebound port {PORT} at monotonic {:.1}", monotonic()));
            }
            continue;
        };
        match lst.accept() {
            Ok((mut s, _)) => {
                let mut req = [0u8; 1024];
                let n = s.read(&mut req).unwrap_or(0);
                let quit = req[..n].starts_with(b"GET /quit");
                let body = format!(
                    "{}\nnow: realtime {:.0}, monotonic {:.1}, alive {:.0}s by monotonic / {:.0}s by realtime, {}\n",
                    log.lock().unwrap().join("\n"),
                    realtime(), monotonic(), monotonic() - mono0, realtime() - real0, ids()
                );
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if quit {
                    toast("bye");
                    return;
                }
            }
            Err(e) => {
                log.lock().unwrap().push(format!("accept failed at monotonic {:.1}: {e}; rebinding", monotonic()));
                listener = None;
            }
        }
    }
}

fn bind() -> Option<TcpListener> {
    match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => Some(l),
        Err(e) => {
            toast(&format!("bind {e}"));
            None
        }
    }
}
