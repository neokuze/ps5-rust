//! Diagnostic for std::fs: works only inside /data/ps5-rust-fs-test (removed
//! at the end) and serves the results on port 9958
//! (curl http://<ps5-ip>:9958), then exits.
use ps5::Toast;
use ps5_std_rt as _;
use std::fmt::Debug;
use std::fs;
use std::io::{Read, Write};
use std::time::{Duration, SystemTime};

const DIR: &str = "/data/ps5-rust-fs-test";

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("fs_probe").send();
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        toast(&format!("panic: {msg}"));
    }));
    toast("started");

    let mut out: Vec<String> = Vec::new();
    let mut check = |name: &str, r: Result<String, String>| {
        out.push(match r {
            Ok(v) => format!("ok   {name}: {v}"),
            Err(e) => format!("FAIL {name}: {e}"),
        })
    };
    fn e<E: Debug>(err: E) -> String { format!("{err:?}") }

    let _ = fs::remove_dir_all(DIR);
    check("create_dir", fs::create_dir(DIR).map(|_| String::new()).map_err(e));

    let file = format!("{DIR}/hello.txt");
    let data = "x".repeat(12345);
    check("write 12345 bytes", fs::write(&file, &data).map(|_| String::new()).map_err(e));
    check("read back", fs::read_to_string(&file).map_err(e).and_then(|s| {
        if s == data { Ok(format!("{} bytes match", s.len())) } else { Err(format!("got {} bytes", s.len())) }
    }));

    check("metadata (stat)", fs::metadata(&file).map_err(e).and_then(|m| {
        let v = format!("len {} file {} dir {} ro {}", m.len(), m.is_file(), m.is_dir(), m.permissions().readonly());
        if m.len() == 12345 && m.is_file() && !m.is_dir() { Ok(v) } else { Err(v) }
    }));
    check("modified time", fs::metadata(&file).and_then(|m| m.modified()).map_err(e).and_then(|t| {
        let age = SystemTime::now().duration_since(t).unwrap_or(Duration::MAX);
        if age < Duration::from_secs(600) { Ok(format!("{age:?} ago")) } else { Err(format!("{t:?}")) }
    }));
    check("File::metadata (fstat)", fs::File::open(&file).and_then(|f| f.metadata()).map_err(e).and_then(|m| {
        if m.len() == 12345 { Ok(format!("len {}", m.len())) } else { Err(format!("len {}", m.len())) }
    }));
    check("dir metadata", fs::metadata(DIR).map_err(e).and_then(|m| {
        use std::os::unix::fs::MetadataExt;
        if m.is_dir() { Ok(format!("is_dir, blksize {}", m.blksize())) } else { Err(format!("{:?}", m.file_type())) }
    }));
    check("exists / missing", Ok(format!("{} / {}", fs::exists(&file).unwrap_or(false), fs::exists(format!("{DIR}/nope")).unwrap_or(true))));

    let _ = fs::create_dir(format!("{DIR}/sub"));
    let _ = fs::write(format!("{DIR}/b.bin"), [1u8, 2, 3]);
    check("read_dir", fs::read_dir(DIR).map_err(e).and_then(|rd| {
        let mut names: Vec<String> = rd
            .map(|en| en.map(|en| {
                let t = en.file_type().map(|t| if t.is_dir() { "/" } else { "" }).unwrap_or("?");
                format!("{}{t}", en.file_name().to_string_lossy())
            }))
            .collect::<Result<_, _>>()
            .map_err(e)?;
        names.sort();
        let v = names.join(" ");
        if v == "b.bin hello.txt sub/" { Ok(v) } else { Err(v) }
    }));
    check("read_dir /", fs::read_dir("/").map_err(e).map(|rd| {
        let names: Vec<String> = rd.filter_map(|en| en.ok()).map(|en| en.file_name().to_string_lossy().into_owned()).collect();
        format!("{} entries: {}", names.len(), names.join(" "))
    }));

    check("copy", fs::copy(&file, format!("{DIR}/copy.txt")).map(|n| format!("{n} bytes")).map_err(e));
    check("rename", fs::rename(format!("{DIR}/copy.txt"), format!("{DIR}/moved.txt")).map(|_| String::new()).map_err(e));
    check("append", fs::OpenOptions::new().append(true).open(format!("{DIR}/moved.txt"))
        .and_then(|mut f| f.write_all(b"END")).and_then(|_| fs::metadata(format!("{DIR}/moved.txt")))
        .map(|m| format!("len {}", m.len())).map_err(e));
    check("canonicalize", fs::canonicalize(format!("{DIR}/sub/../hello.txt")).map(|p| p.display().to_string()).map_err(e));
    check("remove_file", fs::remove_file(format!("{DIR}/b.bin")).map(|_| String::new()).map_err(e));
    check("remove_dir_all", fs::remove_dir_all(DIR).map(|_| String::new()).map_err(e));
    check("gone", Ok(format!("exists={}", fs::exists(DIR).unwrap_or(true))));

    let fails = out.iter().filter(|l| l.starts_with("FAIL")).count();
    serve(&out, fails);
}

fn serve(lines: &[String], fails: usize) {
    let listener = match std::net::TcpListener::bind("0.0.0.0:9958") {
        Ok(l) => l,
        Err(err) => return toast(&format!("bind {err}")),
    };
    toast(&format!("{fails} FAIL - results on port 9958"));
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
