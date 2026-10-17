//! Rest-mode isolation test, step 2: rest_idle plus one listening TCP
//! socket (port 10002, non-blocking, polled from the same loop; still no
//! extra threads). rest_idle survives rest mode; if this one takes the
//! console down, a listening socket is the cause. `curl` on the port shows
//! the gaps seen so far. Exits on the first accept error (after rest the
//! socket stays broken with os error 163) or after 15 minutes.
use ps5::Toast;
use ps5_std_rt as _;
use std::ffi::c_int;
use std::io::{ErrorKind, Read, Write};
use std::time::Duration;

const PORT: u16 = 10002;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(clock: c_int, ts: *mut Timespec) -> c_int;
}

fn now(clock: c_int) -> i64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(clock, &mut ts) };
    ts.sec
}

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("rest_net").send();
}

fn main() {
    let listener = match std::net::TcpListener::bind(("0.0.0.0", PORT)).and_then(|l| l.set_nonblocking(true).map(|_| l)) {
        Ok(l) => l,
        Err(e) => return toast(&format!("bind {e}")),
    };
    toast(&format!("listening on {PORT}; put the console in rest now"));
    let mut log = vec![format!("start: realtime {}, monotonic {}", now(0), now(4))];
    let start = now(4);
    let (mut real, mut mono) = (now(0), now(4));
    while now(4) - start < 15 * 60 {
        std::thread::sleep(Duration::from_secs(1));
        let (r, m) = (now(0), now(4));
        if r - real > 10 || m - mono > 10 {
            let line = format!("gap: real +{}s mono +{}s", r - real, m - mono);
            toast(&line);
            log.push(line);
        }
        (real, mono) = (r, m);
        match listener.accept() {
            Ok((mut s, _)) => {
                let _ = s.set_nonblocking(false);
                let mut req = [0u8; 1024];
                let _ = s.read(&mut req);
                let body = format!("{}\nnow: realtime {}, monotonic {}\n", log.join("\n"), now(0), now(4));
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => {
                let line = format!("accept error: {e}");
                toast(&format!("{line}; exiting"));
                return;
            }
        }
    }
    toast("15 min, exiting");
}
