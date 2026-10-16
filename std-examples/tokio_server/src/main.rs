//! tokio on the PS5: an async HTTP server on port 9956 that handles every
//! connection as its own task, on a multi-threaded runtime.
//!
//!   curl http://<ps5-ip>:9956/          hit counter + uptime
//!   curl http://<ps5-ip>:9956/slow      waits 2 s without blocking others
//!   curl http://<ps5-ip>:9956/quit      stops the server
use ps5::Toast;
use ps5_std_rt as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;

const PORT: u16 = 9956;

fn toast(title: &str, body: &str) {
    let _ = Toast::new(title).sub(body).send();
}

fn main() {
    // Show panics as toasts: otherwise a payload just vanishes.
    std::panic::set_hook(Box::new(|info| {
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        toast("tokio_server panic", &format!("{msg} @ {at}"));
    }));
    toast("tokio_server", "starting runtime");

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => return toast("tokio runtime failed", &e.to_string()),
    };
    rt.block_on(serve());
}

async fn serve() {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)).await {
        Ok(l) => l,
        Err(e) => return toast("tokio bind failed", &e.to_string()),
    };
    toast("tokio_server running", &format!("port {PORT}"));

    let hits = Arc::new(AtomicU64::new(0));
    let quit = Arc::new(Notify::new());
    let start = Instant::now();

    loop {
        let (mut stream, peer) = tokio::select! {
            r = listener.accept() => match r {
                Ok(c) => c,
                Err(_) => continue,
            },
            _ = quit.notified() => break,
        };
        let hits = hits.clone();
        let quit = quit.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let path = std::str::from_utf8(&buf[..n])
                .ok()
                .and_then(|r| r.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_owned();

            let body = match path.as_str() {
                "/slow" => {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    "slept 2 s; other requests kept being served\n".to_owned()
                }
                "/quit" => {
                    quit.notify_one();
                    "bye\n".to_owned()
                }
                _ => {
                    let n = hits.fetch_add(1, Ordering::Relaxed) + 1;
                    format!(
                        "tokio on PS5\nhit #{n} from {peer}\nuptime {:.1} s\nworker {:?}\n",
                        start.elapsed().as_secs_f32(),
                        std::thread::current().id()
                    )
                }
            };
            let _ = stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
        });
    }
    toast("tokio_server stopped", &format!("{} hits", hits.load(Ordering::Relaxed)));
}
