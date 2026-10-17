//! Rest-mode isolation test, step 3: a thread blocked inside accept() while
//! the console goes to rest. rest_idle (no socket) and rest_net (socket
//! polled without blocking) both survive; rest_probe, which sat in a
//! blocking accept, took the console down. Here the main thread blocks in
//! accept on port 10003 and nothing else runs, so a crash points at a
//! blocking socket call during suspend. Exits on the first connection or
//! when accept fails.
use ps5::Toast;
use ps5_std_rt as _;
use std::io::{Read, Write};

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("rest_block").send();
}

fn main() {
    let listener = match std::net::TcpListener::bind("0.0.0.0:10003") {
        Ok(l) => l,
        Err(e) => return toast(&format!("bind {e}")),
    };
    toast("blocked in accept on 10003; put the console in rest now");
    match listener.accept() {
        Ok((mut s, _)) => {
            let mut req = [0u8; 1024];
            let _ = s.read(&mut req);
            let body = "rest_block: accept returned a connection\n";
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            toast("accept ok, exiting");
        }
        Err(e) => toast(&format!("accept error after wake: {e}")),
    }
}
