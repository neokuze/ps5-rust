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

    // libc calls the SDK's libc.a implements or that ps5-std-rt patches.
    {
        use ps5_std_rt::{Dirent12, Stat12};
        use std::ffi::{c_char, c_int, c_void, CStr, CString};
        extern "C" {
            fn fstatat(fd: c_int, path: *const c_char, buf: *mut Stat12, flag: c_int) -> c_int;
            fn mkstemp(template: *mut c_char) -> c_int;
            fn mkdtemp(template: *mut c_char) -> *mut c_char;
            fn scandir(
                dir: *const c_char,
                list: *mut *mut *mut Dirent12,
                select: Option<unsafe extern "C" fn(*const Dirent12) -> c_int>,
                compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
            ) -> c_int;
            fn alphasort(a: *const c_void, b: *const c_void) -> c_int;
            fn fts_open(paths: *const *const c_char, opts: c_int, cmp: *const c_void) -> *mut c_void;
            fn free(p: *mut c_void);
            fn close(fd: c_int) -> c_int;
            fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
        }
        const AT_FDCWD: c_int = -100;
        const AT_SYMLINK_NOFOLLOW: c_int = 0x200;
        let err = || std::io::Error::last_os_error().to_string();
        let cfile = CString::new(file.clone()).unwrap();

        for (name, flag) in [("fstatat", 0), ("fstatat nofollow", AT_SYMLINK_NOFOLLOW)] {
            check(name, unsafe {
                let mut st: Stat12 = std::mem::zeroed();
                if fstatat(AT_FDCWD, cfile.as_ptr(), &mut st, flag) != 0 {
                    Err(err())
                } else {
                    let v = format!("size {} mode {:o} nlink {}", st.size, st.mode, st.nlink);
                    if st.size == 12345 && st.mode & 0o170000 == 0o100000 { Ok(v) } else { Err(v) }
                }
            });
        }

        check("fstatat missing file", unsafe {
            let missing = CString::new(format!("{DIR}/nope")).unwrap();
            let mut st: Stat12 = std::mem::zeroed();
            let ret = fstatat(AT_FDCWD, missing.as_ptr(), &mut st, 0);
            let code = std::io::Error::last_os_error().raw_os_error();
            if ret == -1 && code == Some(2) { Ok("-1 ENOENT".into()) } else { Err(format!("ret {ret} errno {code:?}")) }
        });

        check("mkstemp", unsafe {
            let mut t = format!("{DIR}/tmpXXXXXX\0").into_bytes();
            let fd = mkstemp(t.as_mut_ptr().cast());
            if fd < 0 {
                Err(err())
            } else {
                write(fd, b"abc".as_ptr().cast(), 3);
                close(fd);
                let path = CStr::from_ptr(t.as_ptr().cast()).to_string_lossy().into_owned();
                let len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let _ = fs::remove_file(&path);
                if len == 3 { Ok(path) } else { Err(format!("{path} len {len}")) }
            }
        });
        check("mkdtemp", unsafe {
            let mut t = format!("{DIR}/dirXXXXXX\0").into_bytes();
            let p = mkdtemp(t.as_mut_ptr().cast());
            if p.is_null() {
                Err(err())
            } else {
                let path = CStr::from_ptr(p).to_string_lossy().into_owned();
                let is_dir = fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false);
                let _ = fs::remove_dir(&path);
                if is_dir { Ok(path) } else { Err(format!("{path} not a dir")) }
            }
        });

        check("scandir + alphasort", unsafe {
            let cdir = CString::new(DIR).unwrap();
            let mut list: *mut *mut Dirent12 = std::ptr::null_mut();
            let n = scandir(cdir.as_ptr(), &mut list, None, Some(alphasort));
            if n < 0 {
                Err(err())
            } else {
                let mut names = Vec::new();
                for i in 0..n as usize {
                    let e = *list.add(i);
                    names.push(CStr::from_ptr((*e).name.as_ptr()).to_string_lossy().into_owned());
                    free(e.cast());
                }
                free(list.cast());
                let v = names.join(" ");
                if v == ". .. b.bin hello.txt sub" { Ok(v) } else { Err(v) }
            }
        });

        check("fts_open blocked", unsafe {
            let paths = [cdir_ptr(DIR), std::ptr::null()];
            let p = fts_open(paths.as_ptr(), 0, std::ptr::null());
            let code = std::io::Error::last_os_error().raw_os_error();
            if p.is_null() && code == Some(78) { Ok("ENOSYS".into()) } else { Err(format!("{p:?} {code:?}")) }
        });
    }

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

/// A C string that lives for the rest of the (short-lived) probe.
fn cdir_ptr(s: &str) -> *const std::ffi::c_char {
    std::ffi::CString::new(s).unwrap().into_raw()
}
