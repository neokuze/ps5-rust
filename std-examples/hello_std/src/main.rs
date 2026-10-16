//! First std payload: std::thread, std::sync, std::net and format!.
//! Toasts show each step; then it serves a report on port 9955, once:
//!   python ../examples/pc/... or:  curl http://<ps5-ip>:9955
use ps5::Toast;
use ps5_std_rt as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

fn toast(title: &str, body: &str) {
    let _ = Toast::new(title).sub(body).send();
}

fn main() {
    toast("hello_std", "std started");

    let total = Arc::new(Mutex::new(0u64));
    let handles: Vec<_> = (0..4u64)
        .map(|t| {
            let total = Arc::clone(&total);
            thread::spawn(move || {
                let sum: u64 = (t * 1_000_000..(t + 1) * 1_000_000).sum();
                *total.lock().unwrap() += sum;
                sum
            })
        })
        .collect();
    let sums: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let total = *total.lock().unwrap();
    let ok = total == 7_999_998_000_000;

    let report = format!(
        "hello from std on PS5\nthread sums: {sums:?}\ntotal: {total} ({})\n",
        if ok { "OK" } else { "MISMATCH" }
    );
    println!("{report}");
    toast(if ok { "std threads OK" } else { "std threads MISMATCH" }, &format!("total {total}"));

    // Serve the report to one client, then exit.
    let listener = match TcpListener::bind("0.0.0.0:9955") {
        Ok(l) => l,
        Err(e) => return toast("std net failed", &format!("bind: {e}")),
    };
    toast("std net OK", "listening on port 9955");
    if let Ok((mut stream, peer)) = listener.accept() {
        let mut req = [0u8; 1024];
        let _ = stream.read(&mut req);
        let body = format!("{report}peer: {peer}\n");
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        toast("hello_std done", &format!("served {peer}"));
    }
}
