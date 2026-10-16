//! Diagnostic for DNS and HTTPS: resolves a few names with std
//! (getaddrinfo), then does an HTTPS GET with rustls + ring, verifying the
//! certificate against the Mozilla roots. Results on port 9959
//! (curl http://<ps5-ip>:9959), then exits. Only makes outgoing requests.
use ps5::Toast;
use ps5_std_rt as _;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("https_probe").send();
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

    let mut out = Vec::new();
    for host in ["example.com:443", "github.com:443", "localhost:80", "192.168.1.34:8080"] {
        let t = Instant::now();
        out.push(match host.to_socket_addrs() {
            Ok(addrs) => {
                let v: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
                format!("ok   dns {host}: {} ({:?})", v.join(" "), t.elapsed())
            }
            Err(e) => format!("FAIL dns {host}: {e:?}"),
        });
    }

    for (host, path) in [("example.com", "/"), ("www.rust-lang.org", "/")] {
        out.push(match https_get(host, path) {
            Ok(v) => format!("ok   https {host}{path}: {v}"),
            Err(e) => format!("FAIL https {host}{path}: {e}"),
        });
    }

    let fails = out.iter().filter(|l| l.starts_with("FAIL")).count();
    serve(&out, fails);
}

fn https_get(host: &str, path: &str) -> Result<String, String> {
    let t = Instant::now();
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("config: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = host.to_owned().try_into().map_err(|e| format!("name: {e:?}"))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| format!("tls: {e}"))?;

    let mut sock = TcpStream::connect((host, 443)).map_err(|e| format!("connect: {e}"))?;
    sock.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
    write!(tls, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: ps5-rust\r\n\r\n")
        .map_err(|e| format!("write: {e}"))?;

    let mut body = Vec::new();
    match tls.read_to_end(&mut body) {
        Ok(_) => {}
        // Some servers close without close_notify; we already have the data.
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !body.is_empty() => {}
        Err(e) => return Err(format!("read: {e}")),
    }
    let status = String::from_utf8_lossy(&body).lines().next().unwrap_or("").to_owned();
    let suite = conn.negotiated_cipher_suite().map(|s| format!("{:?}", s.suite())).unwrap_or_default();
    let version = conn.protocol_version().map(|v| format!("{v:?}")).unwrap_or_default();
    Ok(format!("{status} | {} bytes | {version} {suite} | {:?}", body.len(), t.elapsed()))
}

fn serve(lines: &[String], fails: usize) {
    let listener = match std::net::TcpListener::bind("0.0.0.0:9959") {
        Ok(l) => l,
        Err(err) => return toast(&format!("bind {err}")),
    };
    toast(&format!("{fails} FAIL - results on port 9959"));
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
