//! Rest-mode test, step 5: a thread blocked reading a connected socket while
//! the console goes to rest. Run `curl http://<ps5-ip>:10005` and leave it
//! waiting: the payload reads the request, checks that a read timeout still
//! works (ps5-std-rt turns SO_RCVTIMEO into a kevent timeout), then blocks in
//! a plain read on the same connection (nothing more will come). With the
//! kevent-first recv in ps5-std-rt the console should survive; the result
//! comes as a toast after wake.
use ps5::Toast;
use ps5_std_rt as _;
use std::io::{ErrorKind, Read};
use std::time::{Duration, Instant};

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("rest_recv").send();
}

fn main() {
    let listener = match std::net::TcpListener::bind("0.0.0.0:10005") {
        Ok(l) => l,
        Err(e) => return toast(&format!("bind {e}")),
    };
    toast("run curl on 10005 and leave it waiting");
    let (mut s, _) = match listener.accept() {
        Ok(c) => c,
        Err(e) => return toast(&format!("accept {e}")),
    };
    let mut req = [0u8; 1024];
    let _ = s.read(&mut req);

    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
    let t = Instant::now();
    let r = s.read(&mut req);
    let timeout_ok = matches!(&r, Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut));
    toast(&format!("read timeout {}: {:?} after {:?}", if timeout_ok { "ok" } else { "FAIL" }, r.map_err(|e| e.kind()), t.elapsed()));

    let _ = s.set_read_timeout(None);
    toast("blocked in read; put the console in rest now");
    match s.read(&mut req) {
        Ok(n) => toast(&format!("read returned {n} after wake")),
        Err(e) => toast(&format!("read error after wake: {e}")),
    }
}
