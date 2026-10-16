//! Makes Rust's FreeBSD `std` link as a PS5 payload. Add it as a dependency
//! and reference it once so the linker keeps it:
//!
//! ```ignore
//! use ps5_std_rt as _;
//! ```
//!
//! The PS5 kernel is FreeBSD 11 with a few gaps, while Rust's std (and the
//! libc crate) assume FreeBSD 12. This crate covers the difference:
//!
//! - empty stand-ins for libraries std asks the linker for (build.rs);
//! - `accept4`, `pthread_setname_np`, `dl_iterate_phdr`, missing from the SDK;
//! - `kevent`, converting FreeBSD 12 `struct kevent` to the 11 layout;
//! - `fcntl`, emulating `F_DUPFD_CLOEXEC` (rejected by the PS5 kernel);
//! - `stat`/`lstat`/`fstat` and the `DIR` API, converting FreeBSD 11
//!   `struct stat` / `struct dirent` to the 12 layouts.
use core::ffi::{c_char, c_int, c_void};

const F_SETFD: c_int = 2;
const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;
const FD_CLOEXEC: c_int = 1;
const O_NONBLOCK: c_int = 0x4;
const SOCK_CLOEXEC: c_int = 0x1000_0000;
const SOCK_NONBLOCK: c_int = 0x2000_0000;

extern "C" {
    fn accept(fd: c_int, addr: *mut c_void, len: *mut u32) -> c_int;
    fn close(fd: c_int) -> c_int;
}

/// FreeBSD 10+ accept4, as accept + fcntl.
#[no_mangle]
pub unsafe extern "C" fn accept4(fd: c_int, addr: *mut c_void, len: *mut u32, flags: c_int) -> c_int {
    let s = accept(fd, addr, len);
    if s < 0 {
        return s;
    }
    if flags & SOCK_CLOEXEC != 0 && sys_fcntl(s, F_SETFD, FD_CLOEXEC as usize) < 0
        || flags & SOCK_NONBLOCK != 0
            && sys_fcntl(s, F_SETFL, (sys_fcntl(s, F_GETFL, 0) | O_NONBLOCK) as usize) < 0
    {
        close(s);
        return -1;
    }
    s
}

/// Thread names are cosmetic; std ignores the result.
#[no_mangle]
pub extern "C" fn pthread_setname_np(_thread: usize, _name: *const c_char) -> c_int {
    0
}

/// Used by std's backtrace/unwind code, which only runs when printing a
/// panic backtrace. Reporting no loaded objects just gives empty backtraces.
#[no_mangle]
pub extern "C" fn dl_iterate_phdr(_cb: *const c_void, _data: *mut c_void) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// kevent: Rust's libc uses the FreeBSD 12 `struct kevent` (64 bytes, with
// `ext[4]`), but the PS5 kernel is FreeBSD 11 (32 bytes, no `ext`) and
// rejects the new layout (EOPNOTSUPP). The first 32 bytes match, so we copy
// into the old layout and call the FreeBSD 11 kevent syscall directly.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Kevent12 {
    pub ident: usize,
    pub filter: i16,
    pub flags: u16,
    pub fflags: u32,
    pub data: i64,
    pub udata: *mut c_void,
    pub ext: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Kevent11 {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: i64,
    udata: *mut c_void,
}

const SYS_KEVENT: u64 = 363;

extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn __error() -> *mut c_int;
}

/// FreeBSD syscall: on error the carry flag is set and rax holds errno.
unsafe fn syscall6(n: u64, a: [u64; 6]) -> i64 {
    let ret: i64;
    let failed: u8;
    core::arch::asm!(
        "syscall",
        "setc {failed}",
        failed = out(reg_byte) failed,
        inlateout("rax") n as i64 => ret,
        in("rdi") a[0], in("rsi") a[1], in("rdx") a[2],
        in("r10") a[3], in("r8") a[4], in("r9") a[5],
        lateout("rcx") _, lateout("r11") _,
        options(nostack),
    );
    if failed != 0 {
        *__error() = ret as c_int;
        -1
    } else {
        ret
    }
}

#[no_mangle]
pub unsafe extern "C" fn kevent(
    kq: c_int,
    changes: *const Kevent12,
    nchanges: c_int,
    events: *mut Kevent12,
    nevents: c_int,
    timeout: *const c_void,
) -> c_int {
    let nc = nchanges.max(0) as usize;
    let ne = nevents.max(0) as usize;
    let size = core::mem::size_of::<Kevent11>();
    let old_changes = if nc > 0 { malloc(nc * size) as *mut Kevent11 } else { core::ptr::null_mut() };
    let old_events = if ne > 0 { malloc(ne * size) as *mut Kevent11 } else { core::ptr::null_mut() };
    if (nc > 0 && old_changes.is_null()) || (ne > 0 && old_events.is_null()) {
        free(old_changes.cast());
        free(old_events.cast());
        *__error() = 12; // ENOMEM
        return -1;
    }

    for i in 0..nc {
        let c = &*changes.add(i);
        *old_changes.add(i) = Kevent11 {
            ident: c.ident, filter: c.filter, flags: c.flags,
            fflags: c.fflags, data: c.data, udata: c.udata,
        };
    }

    let ret = syscall6(SYS_KEVENT, [
        kq as u64, old_changes as u64, nc as u64,
        old_events as u64, ne as u64, timeout as u64,
    ]);

    // EV_RECEIPT/EV_ERROR results also come back in `events`.
    if ret > 0 {
        for i in 0..(ret as usize).min(ne) {
            let e = *old_events.add(i);
            *events.add(i) = Kevent12 {
                ident: e.ident, filter: e.filter, flags: e.flags,
                fflags: e.fflags, data: e.data, udata: e.udata, ext: [0; 4],
            };
        }
    }
    free(old_changes.cast());
    free(old_events.cast());
    ret as c_int
}

// ---------------------------------------------------------------------------
// fcntl: the PS5 kernel rejects F_DUPFD_CLOEXEC (EOPNOTSUPP), which std uses
// for every try_clone() (and tokio for its I/O driver). Emulate it with
// F_DUPFD + F_SETFD; everything else goes straight to the fcntl syscall.
//
// fcntl is variadic in C, but on x86-64 SysV the optional argument arrives in
// the same register as a fixed third parameter, so this signature is
// call-compatible.

const SYS_FCNTL: u64 = 92;
const F_DUPFD: c_int = 0;
const F_DUPFD_CLOEXEC: c_int = 17;

unsafe fn sys_fcntl(fd: c_int, cmd: c_int, arg: usize) -> c_int {
    syscall6(SYS_FCNTL, [fd as u64, cmd as u64, arg as u64, 0, 0, 0]) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn fcntl(fd: c_int, cmd: c_int, arg: usize) -> c_int {
    if cmd != F_DUPFD_CLOEXEC {
        return sys_fcntl(fd, cmd, arg);
    }
    let new = sys_fcntl(fd, F_DUPFD, arg);
    if new >= 0 && sys_fcntl(new, F_SETFD, FD_CLOEXEC as usize) < 0 {
        let errno = *__error();
        close(new);
        *__error() = errno;
        return -1;
    }
    new
}

// ---------------------------------------------------------------------------
// stat family: the PS5 kernel has the FreeBSD 11 `struct stat` (120 bytes,
// 32-bit ino/dev, 16-bit nlink); Rust's libc uses the FreeBSD 12 one (224
// bytes). Call the FreeBSD 11 syscalls and widen the result.

#[repr(C)]
#[derive(Clone, Copy)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Stat11 {
    dev: u32,
    ino: u32,
    mode: u16,
    nlink: u16,
    uid: u32,
    gid: u32,
    rdev: u32,
    atim: Timespec,
    mtim: Timespec,
    ctim: Timespec,
    size: i64,
    blocks: i64,
    blksize: i32,
    flags: u32,
    gen: u32,
    lspare: i32,
    birthtim: Timespec,
}

/// FreeBSD 12+ `struct stat`, as in the libc crate.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stat12 {
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub mode: u16,
    pad0: i16,
    pub uid: u32,
    pub gid: u32,
    pad1: i32,
    pub rdev: u64,
    pub atime: i64,
    pub atime_nsec: i64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub ctime: i64,
    pub ctime_nsec: i64,
    pub birthtime: i64,
    pub birthtime_nsec: i64,
    pub size: i64,
    pub blocks: i64,
    pub blksize: i32,
    pub flags: u32,
    pub gen: u64,
    spare: [u64; 10],
}

const _: () = assert!(core::mem::size_of::<Stat11>() == 120);
const _: () = assert!(core::mem::size_of::<Stat12>() == 224);

const SYS_STAT11: u64 = 188;
const SYS_FSTAT11: u64 = 189;
const SYS_LSTAT11: u64 = 190;

unsafe fn widen_stat(old: &Stat11, out: *mut Stat12) {
    *out = Stat12 {
        dev: old.dev as u64,
        ino: old.ino as u64,
        nlink: old.nlink as u64,
        mode: old.mode,
        pad0: 0,
        uid: old.uid,
        gid: old.gid,
        pad1: 0,
        rdev: old.rdev as u64,
        atime: old.atim.sec,
        atime_nsec: old.atim.nsec,
        mtime: old.mtim.sec,
        mtime_nsec: old.mtim.nsec,
        ctime: old.ctim.sec,
        ctime_nsec: old.ctim.nsec,
        birthtime: old.birthtim.sec,
        birthtime_nsec: old.birthtim.nsec,
        size: old.size,
        blocks: old.blocks,
        blksize: old.blksize,
        flags: old.flags,
        gen: old.gen as u64,
        spare: [0; 10],
    };
}

/// Runs a FreeBSD 11 stat-style syscall into a temporary and widens it.
unsafe fn stat_call(n: u64, args: [u64; 6], buf_arg: usize, out: *mut Stat12) -> c_int {
    let mut old: Stat11 = core::mem::zeroed();
    let mut a = args;
    a[buf_arg] = &mut old as *mut Stat11 as u64;
    let ret = syscall6(n, a);
    if ret == 0 {
        widen_stat(&old, out);
    }
    ret as c_int
}

#[no_mangle]
pub unsafe extern "C" fn stat(path: *const c_char, buf: *mut Stat12) -> c_int {
    stat_call(SYS_STAT11, [path as u64, 0, 0, 0, 0, 0], 1, buf)
}

#[no_mangle]
pub unsafe extern "C" fn lstat(path: *const c_char, buf: *mut Stat12) -> c_int {
    stat_call(SYS_LSTAT11, [path as u64, 0, 0, 0, 0, 0], 1, buf)
}

#[no_mangle]
pub unsafe extern "C" fn fstat(fd: c_int, buf: *mut Stat12) -> c_int {
    stat_call(SYS_FSTAT11, [fd as u64, 0, 0, 0, 0, 0], 1, buf)
}

// fstatat is not wrapped: the SDK's libc.a defines it in the same object as
// syscalls std needs, so a second definition clashes. std doesn't call it on
// FreeBSD; crates that do still get the FreeBSD 11 layout.

// ---------------------------------------------------------------------------
// Directories: the console's readdir returns FreeBSD 11 `struct dirent`
// (32-bit d_fileno, no d_off); std reads the FreeBSD 12 one. Replacing
// readdir hides the original, so the whole DIR API is implemented here on
// top of the console's getdirentries (FreeBSD 11 records), widening each
// entry.

/// FreeBSD 12+ `struct dirent`, as in the libc crate.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent12 {
    pub fileno: u64,
    pub off: i64,
    pub reclen: u16,
    pub kind: u8,
    pad0: u8,
    pub namlen: u16,
    pad1: u16,
    pub name: [c_char; 256],
}

/// Header of a FreeBSD 11 dirent record (the name follows).
#[repr(C)]
struct Dirent11Head {
    fileno: u32,
    reclen: u16,
    kind: u8,
    namlen: u8,
}

// getdirentries fails with EINVAL if the buffer is smaller than the
// filesystem's block size (e.g. /data), so size it from st_blksize like
// FreeBSD's own opendir does.
const DIRBUF_MIN: usize = 8192;
const DIRBUF_MAX: usize = 1 << 20;
const O_RDONLY: c_int = 0;
const O_DIRECTORY: c_int = 0x0002_0000;
const O_CLOEXEC: c_int = 0x0010_0000;

pub struct Dir {
    fd: c_int,
    buf: *mut u8,
    cap: usize,
    len: usize,
    pos: usize,
    base: i64,
    ent: Dirent12,
}

extern "C" {
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn getdirentries(fd: c_int, buf: *mut c_char, nbytes: usize, basep: *mut i64) -> isize;
}

#[no_mangle]
pub unsafe extern "C" fn fdopendir(fd: c_int) -> *mut Dir {
    let mut st: Stat11 = core::mem::zeroed();
    if syscall6(SYS_FSTAT11, [fd as u64, &mut st as *mut Stat11 as u64, 0, 0, 0, 0]) != 0 {
        return core::ptr::null_mut();
    }
    let cap = (st.blksize.max(0) as usize).clamp(DIRBUF_MIN, DIRBUF_MAX);

    let d = malloc(core::mem::size_of::<Dir>()) as *mut Dir;
    let buf = malloc(cap) as *mut u8;
    if d.is_null() || buf.is_null() {
        free(d.cast());
        free(buf.cast());
        *__error() = 12; // ENOMEM
        return core::ptr::null_mut();
    }
    (*d).fd = fd;
    (*d).buf = buf;
    (*d).cap = cap;
    (*d).len = 0;
    (*d).pos = 0;
    (*d).base = 0;
    sys_fcntl(fd, F_SETFD, FD_CLOEXEC as usize);
    d
}

#[no_mangle]
pub unsafe extern "C" fn opendir(path: *const c_char) -> *mut Dir {
    let fd = open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if fd < 0 {
        return core::ptr::null_mut();
    }
    let d = fdopendir(fd);
    if d.is_null() {
        close(fd);
    }
    d
}

#[no_mangle]
pub unsafe extern "C" fn readdir(d: *mut Dir) -> *mut Dirent12 {
    let d = &mut *d;
    loop {
        if d.pos >= d.len {
            let n = getdirentries(d.fd, d.buf.cast(), d.cap, &mut d.base);
            if n <= 0 {
                return core::ptr::null_mut(); // end (errno untouched) or error
            }
            d.len = (n as usize).min(d.cap);
            d.pos = 0;
        }
        let hsize = core::mem::size_of::<Dirent11Head>();
        if d.pos + hsize > d.len {
            d.pos = d.len; // truncated record: read the next block
            continue;
        }
        let head = &*(d.buf.add(d.pos) as *const Dirent11Head);
        let reclen = head.reclen as usize;
        if reclen < hsize || d.pos + reclen > d.len {
            d.pos = d.len; // corrupt record: read the next block
            continue;
        }
        let name = d.buf.add(d.pos + hsize);
        d.pos += reclen;
        if head.fileno == 0 {
            continue; // deleted entry
        }
        // The name must fit inside its record.
        let n = (head.namlen as usize).min(255).min(reclen - hsize);
        d.ent.fileno = head.fileno as u64;
        d.ent.off = 0;
        d.ent.reclen = core::mem::size_of::<Dirent12>() as u16;
        d.ent.kind = head.kind;
        d.ent.namlen = n as u16;
        core::ptr::copy_nonoverlapping(name, d.ent.name.as_mut_ptr().cast(), n);
        d.ent.name[n] = 0;
        return &mut d.ent;
    }
}

#[no_mangle]
pub unsafe extern "C" fn closedir(d: *mut Dir) -> c_int {
    let r = close((*d).fd);
    free((*d).buf.cast());
    free(d.cast());
    r
}

#[no_mangle]
pub unsafe extern "C" fn dirfd(d: *mut Dir) -> c_int {
    (*d).fd
}

// ---------------------------------------------------------------------------
// getrandom(2) appeared in FreeBSD 12; the getrandom crate (rustls/ring)
// calls it. arc4random_buf, which std already uses on the console, is
// seeded by the kernel and never fails, so it serves every flag.

extern "C" {
    fn arc4random_buf(buf: *mut c_void, n: usize);
}

#[no_mangle]
pub unsafe extern "C" fn getrandom(buf: *mut c_void, len: usize, _flags: u32) -> isize {
    arc4random_buf(buf, len);
    len as isize
}
