//! Diagnostic for reqwest (hyper + tokio + rustls/ring): async GETs over
//! HTTP and HTTPS, a redirect, a timeout, and the blocking client (which
//! runs its own tokio runtime on a thread). Results on port 9962
//! (curl http://<ps5-ip>:9962), then exits. Only makes outgoing requests.
use ps5::Toast;
use ps5_std_rt as _;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("reqwest_probe").send();
}

async fn get(client: &reqwest::Client, url: &str) -> String {
    let t = Instant::now();
    match client.get(url).send().await {
        Ok(r) => {
            let (status, final_url, version) = (r.status(), r.url().to_string(), r.version());
            match r.bytes().await {
                Ok(b) => format!("ok   GET {url}: {status} {version:?} -> {final_url} | {} bytes | {:?}", b.len(), t.elapsed()),
                Err(e) => format!("FAIL GET {url} body: {e:?}"),
            }
        }
        Err(e) => format!("FAIL GET {url}: {e:?}"),
    }
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

    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => return toast(&format!("runtime failed: {e}")),
    };
    let mut out = rt.block_on(async {
        let mut out = Vec::new();
        let client = match reqwest::Client::builder()
            .user_agent("ps5-rust")
            .timeout(Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => return vec![format!("FAIL client build: {e:?}")],
        };
        for url in [
            "http://example.com/",
            "https://example.com/",
            "https://www.rust-lang.org/",
            // http -> https redirect
            "http://github.com/",
        ] {
            out.push(get(&client, url).await);
        }
        // Concurrent requests on one runtime.
        let t = Instant::now();
        let (a, b) = tokio::join!(get(&client, "https://example.com/"), get(&client, "https://www.rust-lang.org/"));
        let ok = a.starts_with("ok") && b.starts_with("ok");
        out.push(format!("{} concurrent x2: {:?}", if ok { "ok  " } else { "FAIL" }, t.elapsed()));
        // Timeout: 10.255.255.1 is unroutable, so connect never completes.
        let t = Instant::now();
        out.push(match client.get("http://10.255.255.1/").timeout(Duration::from_secs(2)).send().await {
            Err(e) if e.is_timeout() => format!("ok   timeout after {:?}", t.elapsed()),
            r => format!("FAIL timeout: {r:?}"),
        });
        out
    });
    drop(rt);

    // The blocking client starts its own runtime on a background thread.
    let t = Instant::now();
    out.push(match reqwest::blocking::Client::builder().user_agent("ps5-rust").build()
        .and_then(|c| c.get("https://example.com/").send())
        .and_then(|r| { let s = r.status(); r.bytes().map(|b| (s, b.len())) })
    {
        Ok((s, n)) => format!("ok   blocking GET https://example.com/: {s} | {n} bytes | {:?}", t.elapsed()),
        Err(e) => format!("FAIL blocking GET: {e:?}"),
    });

    let fails = out.iter().filter(|l| l.starts_with("FAIL")).count();
    serve(&out, fails);
}

fn serve(lines: &[String], fails: usize) {
    let listener = match std::net::TcpListener::bind("0.0.0.0:9962") {
        Ok(l) => l,
        Err(err) => return toast(&format!("bind {err}")),
    };
    toast(&format!("{fails} FAIL - results on port 9962"));
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
