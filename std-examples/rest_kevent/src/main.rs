//! Rest-mode isolation test, step 4: a thread waiting in kevent (tokio's
//! idle state) while the console goes to rest. rest_block showed a thread
//! blocked in accept() powers the console off; this checks whether waiting
//! for the socket through kqueue does too, which is what every tokio server
//! (and reqwest) does. A current-thread runtime with only the I/O driver and
//! no timers, so the runtime sleeps in kevent with no timeout. Port 10004.
//! Exits on the first connection or when accept fails.
use ps5::Toast;
use ps5_std_rt as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("rest_kevent").send();
}

fn main() {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_io().build() {
        Ok(rt) => rt,
        Err(e) => return toast(&format!("runtime failed: {e}")),
    };
    rt.block_on(async {
        let listener = match tokio::net::TcpListener::bind("0.0.0.0:10004").await {
            Ok(l) => l,
            Err(e) => return toast(&format!("bind {e}")),
        };
        toast("waiting in kevent on 10004; put the console in rest now");
        match listener.accept().await {
            Ok((mut s, _)) => {
                let mut req = [0u8; 1024];
                let _ = s.read(&mut req).await;
                let body = "rest_kevent: accept returned a connection\n";
                let _ = s
                    .write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
                    .await;
                toast("accept ok, exiting");
            }
            Err(e) => toast(&format!("accept error after wake: {e}")),
        }
    });
}
