//! A tiny HTTP server on the PS5. Talk to it from the PC with
//! `python examples/pc/ps5_client.py`, or with curl:
//!
//!   curl http://<ps5-ip>:9954/info
//!   curl "http://<ps5-ip>:9954/notify?msg=hola"
//!   curl http://<ps5-ip>:9954/quit
#![no_std]
#![no_main]

use ps5::fmt::{Hex, Line};
use ps5::net::{TcpListener, TcpStream};
use ps5::{hw, log, Toast};

const PORT: u16 = 9954;

#[no_mangle]
pub extern "C" fn main() -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => {
            log!("http_server failed: errno ", e.0);
            let _ = Toast::new("http_server failed").sub(ps5::line!("errno ", e.0).as_str()).send();
            e.0
        }
    }
}

fn run() -> ps5::Result<()> {
    let server = TcpListener::bind(PORT)?;
    let _ = Toast::new("http_server running").sub(ps5::line!("port ", PORT).as_str()).send();
    log!("http_server listening on port ", PORT);

    loop {
        let (mut client, ip) = server.accept()?;
        // One bad client shouldn't stop the server; only /quit does.
        match handle(&mut client) {
            Ok(Flow::Continue) => {}
            Ok(Flow::Quit) => break,
            Err(e) => log!("client ", ip[0], ".", ip[1], ".", ip[2], ".", ip[3], " errno ", e.0),
        }
    }

    let _ = Toast::new("http_server stopped").send();
    Ok(())
}

enum Flow {
    Continue,
    Quit,
}

fn handle(client: &mut TcpStream) -> ps5::Result<Flow> {
    // Read until the end of the headers; the request line is all we use.
    let mut buf = [0u8; 2048];
    let mut len = 0;
    while len < buf.len() && !buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
        match client.read(&mut buf[len..])? {
            0 => break,
            n => len += n,
        }
    }

    // "GET /path?query HTTP/1.1"
    let line = buf[..len].split(|&b| b == b'\r').next().unwrap_or(&[]);
    let mut parts = line.split(|&b| b == b' ');
    let method = parts.next().unwrap_or(&[]);
    let target = parts.next().unwrap_or(&[]);
    let (path, query) = match target.iter().position(|&b| b == b'?') {
        Some(i) => (&target[..i], &target[i + 1..]),
        None => (target, &[][..]),
    };

    if method != b"GET" {
        respond(client, "405 Method Not Allowed", b"only GET\n")?;
        return Ok(Flow::Continue);
    }

    match path {
        b"/info" => {
            let model = hw::model_name()?;
            let temp = hw::cpu_temperature().unwrap_or(-1);
            let body = ps5::line!(
                "model=", model, "\ncpu_temp=", temp, "C\nfw=", Hex(ps5::fw_version() as u64), "\n"
            );
            respond(client, "200 OK", body.as_bytes())?;
        }
        b"/notify" => {
            let mut msg = Line::<200>::new();
            url_decode(query_param(query, b"msg").unwrap_or(b"(empty)"), &mut msg);
            Toast::new("Message from PC").sub(msg.as_str()).send()?;
            respond(client, "200 OK", b"shown\n")?;
        }
        b"/quit" => {
            respond(client, "200 OK", b"bye\n")?;
            return Ok(Flow::Quit);
        }
        _ => respond(client, "404 Not Found", b"routes: /info /notify?msg=... /quit\n")?,
    }
    Ok(Flow::Continue)
}

fn respond(client: &mut TcpStream, status: &str, body: &[u8]) -> ps5::Result<()> {
    let head = ps5::line!(
        "HTTP/1.1 ", status,
        "\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: ", body.len(),
        "\r\nConnection: close\r\n\r\n"
    );
    client.write_all(head.as_bytes())?;
    client.write_all(body)
}

/// Value of `key` in `a=1&key=value`.
fn query_param<'a>(query: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    query.split(|&b| b == b'&').find_map(|pair| {
        let eq = pair.iter().position(|&b| b == b'=')?;
        (&pair[..eq] == key).then(|| &pair[eq + 1..])
    })
}

/// Decodes `%XX` and `+` from a URL query value.
fn url_decode<const N: usize>(src: &[u8], out: &mut Line<N>) {
    let hex = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let mut i = 0;
    while i < src.len() {
        if src[i] == b'%' && i + 2 < src.len() {
            if let (Some(h), Some(l)) = (hex(src[i + 1]), hex(src[i + 2])) {
                out.push_bytes(&[h << 4 | l]);
                i += 3;
                continue;
            }
        }
        out.push_bytes(&[if src[i] == b'+' { b' ' } else { src[i] }]);
        i += 1;
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
