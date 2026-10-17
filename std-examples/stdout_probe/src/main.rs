//! Diagnostic for stdout/stderr: what fds 0-2 are when a payload starts,
//! what writing to them returns, which console-like devices exist in /dev
//! (only listed, never opened), and whether `println!` can be redirected to
//! a TCP socket with dup2. Writes nothing to the filesystem.
//! Results on port 10000 (curl http://<ps5-ip>:10000), then exits.
use ps5::Toast;
use ps5_std_rt as _;
use std::ffi::{c_int, c_void};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt};

extern "C" {
    fn dup(fd: c_int) -> c_int;
    fn dup2(from: c_int, to: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn isatty(fd: c_int) -> c_int;
}

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("stdout_probe").send();
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// What kind of file an fd is, without taking ownership of it.
fn describe(fd: c_int) -> String {
    let f = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    let flags = unsafe { fcntl(fd, 3 /* F_GETFL */) };
    if flags == -1 {
        return format!("closed/invalid (errno {})", errno());
    }
    match f.metadata() {
        Ok(m) => {
            let t = m.file_type();
            let kind = if t.is_char_device() { "char device" } else if t.is_fifo() { "pipe" }
                else if t.is_socket() { "socket" } else if t.is_file() { "regular file" }
                else if t.is_dir() { "directory" } else { "other" };
            format!("{kind}, mode {:o}, rdev {:#x}, ino {}, flags {flags:#x}, isatty {}",
                m.mode(), m.rdev(), m.ino(), unsafe { isatty(fd) })
        }
        Err(e) => format!("open (flags {flags:#x}) but fstat failed: {e}"),
    }
}

fn main() {
    toast("started");
    let mut out = Vec::new();

    for fd in 0..3 {
        out.push(format!("info fd {fd}: {}", describe(fd)));
    }
    for fd in [1, 2] {
        let msg = b"ps5 stdout_probe raw write\n";
        let r = unsafe { write(fd, msg.as_ptr().cast(), msg.len()) };
        out.push(format!("info write(fd {fd}, {} bytes) = {r}{}", msg.len(),
            if r < 0 { format!(", errno {}", errno()) } else { String::new() }));
    }
    // What println! itself reports (it would panic on error; write! doesn't).
    out.push(format!("info std stdout write: {:?}", writeln!(std::io::stdout(), "ps5 stdout_probe println").and_then(|_| std::io::stdout().flush())));
    out.push(format!("info std stderr write: {:?}", writeln!(std::io::stderr(), "ps5 stdout_probe eprintln")));

    // Only list names; nothing in /dev is opened.
    match std::fs::read_dir("/dev") {
        Ok(rd) => {
            let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            let interesting: Vec<&String> = names.iter()
                .filter(|n| ["console", "tty", "deci", "klog", "stdout", "stderr", "log", "null"].iter().any(|k| n.contains(k)))
                .collect();
            out.push(format!("info /dev: {} entries; console-like: {interesting:?}", names.len()));
        }
        Err(e) => out.push(format!("info /dev not listable: {e}")),
    }

    let listener = match std::net::TcpListener::bind("0.0.0.0:10000") {
        Ok(l) => l,
        Err(err) => return toast(&format!("bind {err}")),
    };
    toast("results on port 10000");
    let Ok((mut s, _)) = listener.accept() else { return };
    let mut req = [0u8; 1024];
    let _ = s.read(&mut req);
    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n");

    // Redirect: point fds 1 and 2 at the client's socket, print, restore.
    let sock = s.as_raw_fd();
    let (old1, old2) = unsafe { (dup(1), dup(2)) };
    let r1 = unsafe { dup2(sock, 1) };
    let r2 = unsafe { dup2(sock, 2) };
    println!("--- the next two lines are println!/eprintln! sent through fd 1/2 ---");
    println!("hello from println! (dup2 to socket: {r1})");
    eprintln!("hello from eprintln! (dup2 to socket: {r2})");
    let _ = std::io::stdout().flush();
    unsafe {
        if old1 >= 0 { dup2(old1, 1); close(old1); } else { close(1); }
        if old2 >= 0 { dup2(old2, 2); close(old2); } else { close(2); }
    }
    out.push(format!("info after restore, fd 1: {}", describe(1)));

    let _ = write!(s, "--- probe results ---\n{}\n", out.join("\n"));
    drop(s);
    toast("results sent");
}
