//! Diagnostic for UDP: std::net::UdpSocket and tokio::net::UdpSocket over
//! loopback, a connected socket, the read timeout (ps5-std-rt's recvfrom
//! turns SO_RCVTIMEO into a kevent timeout), broadcast, and a real DNS query
//! to 1.1.1.1:53 sent by hand. Results on TCP port 10006
//! (curl http://<ps5-ip>:10006), then exits. Writes nothing to disk.
use ps5::Toast;
use ps5_std_rt as _;
use std::io::{ErrorKind, Read, Write};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("udp_probe").send();
}

fn line(name: &str, r: Result<String, String>) -> String {
    match r {
        Ok(v) => format!("ok   {name}: {v}"),
        Err(e) => format!("FAIL {name}: {e}"),
    }
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn loopback() -> Result<String, String> {
    let a = UdpSocket::bind("127.0.0.1:0").map_err(s)?;
    let b = UdpSocket::bind("127.0.0.1:0").map_err(s)?;
    a.set_read_timeout(Some(Duration::from_secs(2))).map_err(s)?;
    b.send_to(b"ping", a.local_addr().map_err(s)?).map_err(s)?;
    let mut buf = [0u8; 64];
    let (n, from) = a.recv_from(&mut buf).map_err(s)?;
    if &buf[..n] != b"ping" || from != b.local_addr().map_err(s)? {
        return Err(format!("got {:?} from {from}", &buf[..n]));
    }
    Ok(format!("4 bytes from {from}"))
}

fn connected() -> Result<String, String> {
    let a = UdpSocket::bind("127.0.0.1:0").map_err(s)?;
    let b = UdpSocket::bind("127.0.0.1:0").map_err(s)?;
    a.connect(b.local_addr().map_err(s)?).map_err(s)?;
    b.connect(a.local_addr().map_err(s)?).map_err(s)?;
    b.set_read_timeout(Some(Duration::from_secs(2))).map_err(s)?;
    a.send(b"hello").map_err(s)?;
    let mut buf = [0u8; 64];
    let n = b.peek(&mut buf).map_err(|e| format!("peek: {e}"))?;
    if &buf[..n] != b"hello" {
        return Err(format!("peek got {:?}", &buf[..n]));
    }
    let n = b.recv(&mut buf).map_err(s)?;
    if &buf[..n] != b"hello" {
        return Err(format!("got {:?}", &buf[..n]));
    }
    Ok("peek then recv 5 bytes".into())
}

fn timeout() -> Result<String, String> {
    let a = UdpSocket::bind("127.0.0.1:0").map_err(s)?;
    a.set_read_timeout(Some(Duration::from_millis(300))).map_err(s)?;
    let t = Instant::now();
    let mut buf = [0u8; 16];
    match a.recv_from(&mut buf) {
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            let el = t.elapsed();
            if el >= Duration::from_millis(250) && el < Duration::from_millis(1000) {
                Ok(format!("{:?} after {el:?}", e.kind()))
            } else {
                Err(format!("{:?} but after {el:?} (want ~300ms)", e.kind()))
            }
        }
        r => Err(format!("{r:?}")),
    }
}

fn nonblocking() -> Result<String, String> {
    let a = UdpSocket::bind("127.0.0.1:0").map_err(s)?;
    a.set_nonblocking(true).map_err(s)?;
    let mut buf = [0u8; 16];
    match a.recv_from(&mut buf) {
        Err(e) if e.kind() == ErrorKind::WouldBlock => Ok("WouldBlock at once".into()),
        r => Err(format!("{r:?}")),
    }
}

fn broadcast() -> Result<String, String> {
    let rx = UdpSocket::bind("0.0.0.0:10007").map_err(|e| format!("bind 10007: {e}"))?;
    rx.set_read_timeout(Some(Duration::from_secs(2))).map_err(s)?;
    let tx = UdpSocket::bind("0.0.0.0:0").map_err(s)?;
    tx.set_broadcast(true).map_err(s)?;
    let sent = tx.send_to(b"ps5-bcast", "255.255.255.255:10007").map_err(|e| format!("send: {e}"))?;
    let mut buf = [0u8; 64];
    Ok(match rx.recv_from(&mut buf) {
        Ok((n, from)) => format!("sent {sent}, own broadcast came back: {n} bytes from {from}"),
        Err(e) => format!("sent {sent}; own broadcast not looped back ({e}), fine on many stacks"),
    })
}

/// A hand-made DNS query for example.com (type A) to 1.1.1.1.
fn dns() -> Result<String, String> {
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(s)?;
    sock.set_read_timeout(Some(Duration::from_secs(3))).map_err(s)?;
    let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in ["example", "com"] {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    let t = Instant::now();
    sock.send_to(&q, "1.1.1.1:53").map_err(|e| format!("send: {e}"))?;
    let mut buf = [0u8; 512];
    let (n, from) = sock.recv_from(&mut buf).map_err(|e| format!("recv: {e}"))?;
    if n < 12 || buf[0..2] != [0x12, 0x34] {
        return Err(format!("bad reply, {n} bytes"));
    }
    let answers = u16::from_be_bytes([buf[6], buf[7]]);
    // With A answers, the last answer's address is the last 4 bytes.
    let ip = if answers > 0 { format!("{}.{}.{}.{}", buf[n - 4], buf[n - 3], buf[n - 2], buf[n - 1]) } else { "-".into() };
    Ok(format!("{n} bytes from {from}, {answers} answer(s), last {ip}, {:?}", t.elapsed()))
}

async fn tokio_udp() -> Result<String, String> {
    let a = tokio::net::UdpSocket::bind("127.0.0.1:0").await.map_err(s)?;
    let b = tokio::net::UdpSocket::bind("127.0.0.1:0").await.map_err(s)?;
    let addr = a.local_addr().map_err(s)?;
    let recv = async {
        let mut buf = [0u8; 64];
        a.recv_from(&mut buf).await.map(|(n, f)| (buf[..n].to_vec(), f))
    };
    let (r, sent) = tokio::join!(recv, b.send_to(b"async", addr));
    sent.map_err(s)?;
    let (data, from) = r.map_err(s)?;
    if data != b"async" {
        return Err(format!("got {data:?}"));
    }
    // Nothing arrives: the tokio timer must cut the wait.
    let t = Instant::now();
    let mut buf = [0u8; 16];
    match tokio::time::timeout(Duration::from_millis(300), a.recv_from(&mut buf)).await {
        Err(_) => Ok(format!("5 bytes from {from}; timeout after {:?}", t.elapsed())),
        Ok(r) => Err(format!("timeout test returned {r:?}")),
    }
}

fn main() {
    toast("started");
    let mut out = vec![
        line("std loopback send_to/recv_from", loopback()),
        line("std connected peek/recv", connected()),
        line("std read timeout 300ms", timeout()),
        line("std non-blocking recv", nonblocking()),
        line("std broadcast", broadcast()),
        line("std DNS query to 1.1.1.1", dns()),
    ];
    out.push(match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => line("tokio UdpSocket", rt.block_on(tokio_udp())),
        Err(e) => format!("FAIL tokio runtime: {e}"),
    });
    let fails = out.iter().filter(|l| l.starts_with("FAIL")).count();
    serve(&out, fails);
}

fn serve(lines: &[String], fails: usize) {
    let listener = match std::net::TcpListener::bind("0.0.0.0:10006") {
        Ok(l) => l,
        Err(err) => return toast(&format!("bind {err}")),
    };
    toast(&format!("{fails} FAIL - results on port 10006"));
    if let Ok((mut s, _)) = listener.accept() {
        let mut req = [0u8; 1024];
        let _ = s.read(&mut req);
        let body = lines.join("\n") + "\n";
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        toast("results sent");
    }
}
