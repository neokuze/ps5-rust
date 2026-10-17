//! Diagnostic for syscall error reporting: provokes failures (missing paths,
//! bad fds, refused connections) and checks they come back as -1 + errno /
//! io::Error. Writes only a symlink in /data/ps5-rust-err-probe-tmp, removed
//! again. Results on port 9960
//! (curl http://<ps5-ip>:9960), then exits.
use ps5::Toast;
use ps5_std_rt as _;
use std::ffi::{c_char, c_int, c_long, CString};
use std::io::{ErrorKind, Read, Write};

extern "C" {
    fn dup(fd: c_int) -> c_int;
    fn openat(fd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn unlinkat(fd: c_int, path: *const c_char, flags: c_int) -> c_int;
    fn mkdirat(fd: c_int, path: *const c_char, mode: u16) -> c_int;
    fn renameat(fd: c_int, from: *const c_char, tfd: c_int, to: *const c_char) -> c_int;
    fn faccessat(fd: c_int, path: *const c_char, mode: c_int, flags: c_int) -> c_int;
    fn readlinkat(fd: c_int, path: *const c_char, buf: *mut c_char, n: usize) -> isize;
    fn syscall(n: c_long, ...) -> c_int;
}

/// What the SDK's syscalls.o stubs do: `syscall` and return rax, without
/// looking at the carry flag. Here for FreeBSD 11 fstatat (493).
unsafe fn sdk_style_fstatat(fd: c_int, path: *const c_char, buf: *mut u8, flag: c_int) -> i64 {
    let ret: i64;
    std::arch::asm!(
        "syscall",
        inlateout("rax") 493i64 => ret,
        in("rdi") fd as i64, in("rsi") path, in("rdx") buf, in("r10") flag as i64,
        lateout("rcx") _, lateout("r11") _,
        options(nostack),
    );
    ret
}

const AT_FDCWD: c_int = -100;
const EBADF: i32 = 9;
const ENOENT: i32 = 2;
const EINVAL: i32 = 22;
const ENOSYS: i32 = 78;

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("err_probe").send();
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// A libc call that must fail: expects -1 and the given errno.
fn expect_err(out: &mut Vec<String>, name: &str, ret: i64, want: i32) {
    let got = errno();
    out.push(if ret == -1 && got == want {
        format!("ok   {name}: -1 errno {got}")
    } else {
        format!("FAIL {name}: returned {ret}, errno {got} (want -1 errno {want})")
    });
}

/// A libc call that must succeed (return 0).
fn expect_ok(out: &mut Vec<String>, name: &str, ret: i64) {
    out.push(if ret == 0 { format!("ok   {name}: 0") } else { format!("FAIL {name}: returned {ret}, errno {}", errno()) });
}

/// A std call that must fail with the given kind.
fn expect_kind<T: std::fmt::Debug>(out: &mut Vec<String>, name: &str, r: std::io::Result<T>, want: ErrorKind) {
    out.push(match r {
        Err(e) if e.kind() == want => format!("ok   {name}: {e}"),
        Err(e) => format!("FAIL {name}: {e} (want {want:?})"),
        Ok(v) => format!("FAIL {name}: succeeded with {v:?} (want {want:?})"),
    });
}

fn main() {
    toast("started");
    let mut out = Vec::new();
    let missing = CString::new("/data/ps5-rust-err-probe-missing/x").unwrap();
    let missing2 = CString::new("/data/ps5-rust-err-probe-missing/y").unwrap();

    unsafe {
        // The bug being fixed: the SDK's stub returns errno as a positive value.
        let mut buf = [0u8; 256];
        let r = sdk_style_fstatat(AT_FDCWD, missing.as_ptr(), buf.as_mut_ptr(), 0);
        out.push(format!("info SDK-style stub (no carry check) on a missing file returns {r} (should be -1)"));

        expect_err(&mut out, "dup(-1)", dup(-1) as i64, EBADF);
        expect_err(&mut out, "openat missing", openat(AT_FDCWD, missing.as_ptr(), 0) as i64, ENOENT);
        expect_err(&mut out, "unlinkat missing", unlinkat(AT_FDCWD, missing.as_ptr(), 0) as i64, ENOENT);
        expect_err(&mut out, "mkdirat missing parent", mkdirat(AT_FDCWD, missing.as_ptr(), 0o755) as i64, ENOENT);
        expect_err(&mut out, "renameat missing", renameat(AT_FDCWD, missing.as_ptr(), AT_FDCWD, missing2.as_ptr()) as i64, ENOENT);
        let mut lb = [0 as c_char; 64];
        let root = CString::new("/").unwrap();
        // The PS5 kernel lacks these two *at syscalls; ps5-std-rt emulates
        // them with access/eaccess/readlink for AT_FDCWD and absolute paths.
        expect_err(&mut out, "faccessat missing", faccessat(AT_FDCWD, missing.as_ptr(), 0, 0) as i64, ENOENT);
        let data = CString::new("/data").unwrap();
        expect_ok(&mut out, "faccessat /data F_OK", faccessat(AT_FDCWD, data.as_ptr(), 0, 0) as i64);
        {
            extern "C" {
                fn getuid() -> u32;
                fn geteuid() -> u32;
                fn getgid() -> u32;
                fn getegid() -> u32;
            }
            out.push(format!(
                "info ids: uid {} euid {} gid {} egid {}",
                getuid(), geteuid(), getgid(), getegid()
            ));
        }
        let r = faccessat(AT_FDCWD, data.as_ptr(), 0, 0x100) as i64;
        out.push(match (r, errno()) {
            (0, _) => "ok   faccessat /data AT_EACCESS: 0".into(),
            (-1, ENOSYS) => "ok   faccessat /data AT_EACCESS: ENOSYS (no eaccess and real != effective ids)".into(),
            (r, e) => format!("FAIL faccessat /data AT_EACCESS: returned {r}, errno {e}"),
        });
        let rel = CString::new("data").unwrap();
        let rootfd = openat(AT_FDCWD, root.as_ptr(), 0);
        expect_err(&mut out, "faccessat relative to dir fd (unsupported)", faccessat(rootfd, rel.as_ptr(), 0, 0) as i64, ENOSYS);
        if rootfd >= 0 {
            std::os::fd::OwnedFd::from_raw_fd_checked(rootfd);
        }
        expect_err(&mut out, "readlinkat not-a-link", readlinkat(AT_FDCWD, root.as_ptr(), lb.as_mut_ptr(), 64) as i64, EINVAL);
        expect_err(&mut out, "syscall(SYS_dup, -1)", syscall(41, -1 as c_int) as i64, EBADF);

        // Success paths still work.
        let fd = dup(1);
        out.push(if fd >= 0 { format!("ok   dup(1): {fd}") } else { format!("FAIL dup(1): {fd} errno {}", errno()) });
        if fd >= 0 {
            std::os::fd::OwnedFd::from_raw_fd_checked(fd);
        }
    }

    // readlinkat / fs::read_link on a real symlink, in a scratch dir that is
    // removed again (the only thing this probe writes).
    let tmp = "/data/ps5-rust-err-probe-tmp";
    let _ = std::fs::remove_dir_all(tmp);
    match std::fs::create_dir(tmp).and_then(|_| std::os::unix::fs::symlink("target-text", format!("{tmp}/link"))) {
        // /data's filesystem has no symlinks (EOPNOTSUPP): nothing to read.
        Err(e) if e.raw_os_error() == Some(45) => {
            out.push("info /data doesn't support symlinks (EOPNOTSUPP); readlinkat on a real link not tested".into())
        }
        Err(e) => out.push(format!("FAIL symlink setup: {e}")),
        Ok(()) => {
            let link = CString::new(format!("{tmp}/link")).unwrap();
            let mut b = [0u8; 64];
            let n = unsafe { readlinkat(AT_FDCWD, link.as_ptr(), b.as_mut_ptr().cast(), b.len()) };
            out.push(if n >= 0 && &b[..n as usize] == b"target-text" {
                "ok   readlinkat symlink: target-text".to_string()
            } else {
                format!("FAIL readlinkat symlink: {n} errno {}", errno())
            });
            out.push(match std::fs::read_link(format!("{tmp}/link")) {
                Ok(p) if p.as_os_str() == "target-text" => "ok   fs::read_link: target-text".to_string(),
                r => format!("FAIL fs::read_link: {r:?}"),
            });
        }
    }
    out.push(match std::fs::remove_dir_all(tmp) {
        Ok(()) if !std::path::Path::new(tmp).exists() => "ok   scratch dir removed".to_string(),
        r => format!("FAIL scratch dir cleanup: {r:?}"),
    });

    let dir = "/data/ps5-rust-err-probe-missing";
    expect_kind(&mut out, "fs::remove_dir_all missing", std::fs::remove_dir_all(dir), ErrorKind::NotFound);
    expect_kind(&mut out, "fs::File::open missing", std::fs::File::open(format!("{dir}/x")), ErrorKind::NotFound);
    expect_kind(&mut out, "fs::metadata missing", std::fs::metadata(format!("{dir}/x")), ErrorKind::NotFound);
    expect_kind(&mut out, "fs::read_dir missing", std::fs::read_dir(dir).map(|_| ()), ErrorKind::NotFound);
    expect_kind(&mut out, "fs::create_dir missing parent", std::fs::create_dir(format!("{dir}/a/b")), ErrorKind::NotFound);
    expect_kind(&mut out, "fs::rename missing", std::fs::rename(format!("{dir}/x"), format!("{dir}/y")), ErrorKind::NotFound);
    expect_kind(&mut out, "fs::remove_file missing", std::fs::remove_file(format!("{dir}/x")), ErrorKind::NotFound);
    // Nothing listens on port 1 of the PS5 itself.
    expect_kind(&mut out, "TcpStream::connect refused", std::net::TcpStream::connect("127.0.0.1:1"), ErrorKind::ConnectionRefused);
    expect_kind(&mut out, "TcpListener::bind in use", {
        let a = std::net::TcpListener::bind("0.0.0.0:9961");
        let b = std::net::TcpListener::bind("0.0.0.0:9961");
        drop(a);
        b
    }, ErrorKind::AddrInUse);

    let fails = out.iter().filter(|l| l.starts_with("FAIL")).count();
    serve(&out, fails);
}

trait FromRawChecked {
    fn from_raw_fd_checked(fd: c_int);
}
impl FromRawChecked for std::os::fd::OwnedFd {
    /// Takes ownership so the dup'd fd is closed.
    fn from_raw_fd_checked(fd: c_int) {
        use std::os::fd::FromRawFd;
        drop(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) });
    }
}

fn serve(lines: &[String], fails: usize) {
    let listener = match std::net::TcpListener::bind("0.0.0.0:9960") {
        Ok(l) => l,
        Err(err) => return toast(&format!("bind {err}")),
    };
    toast(&format!("{fails} FAIL - results on port 9960"));
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
