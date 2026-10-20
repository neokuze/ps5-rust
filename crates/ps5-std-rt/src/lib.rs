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
//! - `stat`/`lstat`/`fstat`/`fstatat`, `scandir` and the `DIR` API,
//!   converting FreeBSD 11 `struct stat` / `struct dirent` to the 12 layouts;
//! - a patched copy of the SDK libc.a (see build.rs) so its own code that
//!   uses the 11 layouts keeps them, and `fts_*` stubs that keep the SDK's
//!   fts (which would hand FreeBSD 11 structs to Rust) out of the link.
//! - `getrandom` (FreeBSD 12+), over `arc4random_buf`;
//! - the SDK's raw syscall stubs, regenerated with error checking (build.rs);
//! - `faccessat`/`readlinkat`, which the PS5 kernel lacks, over access/readlink.
//! - `clock_nanosleep` (ENOSYS on the PS5), over nanosleep;
//! - a blocking `accept` that waits in kevent first: a thread asleep in
//!   accept when the console enters rest mode powers it off; recv,
//!   recvfrom and recvmsg wait the same way.
use core::ffi::{c_char, c_int, c_void};

const F_SETFD: c_int = 2;
const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;
const FD_CLOEXEC: c_int = 1;
const O_NONBLOCK: c_int = 0x4;
const SOCK_CLOEXEC: c_int = 0x1000_0000;
const SOCK_NONBLOCK: c_int = 0x2000_0000;

extern "C" {
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
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
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
pub struct Timespec {
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

// fstatat: the SDK's lives in syscalls.o, which never gets linked (build.rs
// regenerates its stubs and leaves this one out), so this name is free.
const SYS_FSTATAT11: u64 = 493;

#[no_mangle]
pub unsafe extern "C" fn fstatat(fd: c_int, path: *const c_char, buf: *mut Stat12, flag: c_int) -> c_int {
    stat_call(SYS_FSTATAT11, [fd as u64, path as u64, 0, flag as u64, 0, 0], 2, buf)
}

// The SDK's own mktemp.o was compiled for the FreeBSD 11 `struct stat`;
// build.rs points its stat/lstat calls here instead of at the wrappers above.
#[no_mangle]
unsafe extern "C" fn __ps5_stat11(path: *const c_char, buf: *mut c_void) -> c_int {
    syscall6(SYS_STAT11, [path as u64, buf as u64, 0, 0, 0, 0]) as c_int
}

#[no_mangle]
unsafe extern "C" fn __ps5_lstat11(path: *const c_char, buf: *mut c_void) -> c_int {
    syscall6(SYS_LSTAT11, [path as u64, buf as u64, 0, 0, 0, 0]) as c_int
}

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

// ---------------------------------------------------------------------------
// scandir/alphasort: the SDK's scandir.o builds FreeBSD 11 dirents and hands
// them to the caller, who reads them as FreeBSD 12. Defining them here keeps
// that object out of the link; this version sits on the readdir above.

extern "C" {
    fn qsort(base: *mut c_void, n: usize, size: usize, cmp: unsafe extern "C" fn(*const c_void, *const c_void) -> c_int);
    fn strcoll(a: *const c_char, b: *const c_char) -> c_int;
}

type DirentCmp = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;

#[no_mangle]
pub unsafe extern "C" fn scandir(
    path: *const c_char,
    namelist: *mut *mut *mut Dirent12,
    select: Option<unsafe extern "C" fn(*const Dirent12) -> c_int>,
    compar: Option<DirentCmp>,
) -> c_int {
    let d = opendir(path);
    if d.is_null() {
        return -1;
    }
    let mut list: *mut *mut Dirent12 = core::ptr::null_mut();
    let mut len = 0usize;
    let mut cap = 0usize;
    let fail = |list: *mut *mut Dirent12, len: usize, d: *mut Dir| {
        for i in 0..len {
            free((*list.add(i)).cast());
        }
        free(list.cast());
        closedir(d);
        *__error() = 12; // ENOMEM
        -1
    };
    loop {
        let e = readdir(d);
        if e.is_null() {
            break;
        }
        if let Some(sel) = select {
            if sel(e) == 0 {
                continue;
            }
        }
        if len == cap {
            cap = if cap == 0 { 32 } else { cap * 2 };
            let grown = realloc(list.cast(), cap * core::mem::size_of::<*mut Dirent12>()) as *mut *mut Dirent12;
            if grown.is_null() {
                return fail(list, len, d);
            }
            list = grown;
        }
        let copy = malloc(core::mem::size_of::<Dirent12>()) as *mut Dirent12;
        if copy.is_null() {
            return fail(list, len, d);
        }
        *copy = *e;
        *list.add(len) = copy;
        len += 1;
    }
    closedir(d);
    if let (Some(cmp), true) = (compar, len > 1) {
        qsort(list.cast(), len, core::mem::size_of::<*mut Dirent12>(), cmp);
    }
    *namelist = list;
    len as c_int
}

#[no_mangle]
pub unsafe extern "C" fn alphasort(a: *const c_void, b: *const c_void) -> c_int {
    let a = *(a as *const *const Dirent12);
    let b = *(b as *const *const Dirent12);
    strcoll((*a).name.as_ptr(), (*b).name.as_ptr())
}

// ---------------------------------------------------------------------------
// fts: the SDK's fts.o returns FreeBSD 11 `struct stat`/dirent data inside
// FTSENT, which Rust code would misread. It's large and rarely used (std
// doesn't), so these stand-ins keep it out of the link and fail cleanly
// with ENOSYS instead of corrupting memory.

const ENOSYS: c_int = 78;

unsafe fn enosys() {
    *__error() = ENOSYS;
}

#[no_mangle]
pub unsafe extern "C" fn fts_open(_: *const c_void, _: c_int, _: *const c_void) -> *mut c_void {
    enosys();
    core::ptr::null_mut()
}
#[no_mangle]
pub unsafe extern "C" fn fts_read(_: *mut c_void) -> *mut c_void {
    enosys();
    core::ptr::null_mut()
}
#[no_mangle]
pub unsafe extern "C" fn fts_children(_: *mut c_void, _: c_int) -> *mut c_void {
    enosys();
    core::ptr::null_mut()
}
#[no_mangle]
pub unsafe extern "C" fn fts_close(_: *mut c_void) -> c_int {
    enosys();
    -1
}
#[no_mangle]
pub unsafe extern "C" fn fts_set(_: *mut c_void, _: *mut c_void, _: c_int) -> c_int {
    enosys();
    -1
}
#[no_mangle]
pub extern "C" fn fts_get_clientptr(_: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}
#[no_mangle]
pub extern "C" fn fts_get_stream(_: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}
#[no_mangle]
pub extern "C" fn fts_set_clientptr(_: *mut c_void, _: *mut c_void) {}

// ---------------------------------------------------------------------------
// The SDK's raw syscall stubs, regenerated with error checking (see
// build.rs, part 3).
core::arch::global_asm!(include_str!(concat!(env!("OUT_DIR"), "/syscalls.s")), options(att_syntax, raw));

// ---------------------------------------------------------------------------
// faccessat/readlinkat: the PS5 kernel returns ENOSYS for these *at syscalls
// (489, 500). For the usual cases (AT_FDCWD or an absolute path) the plain
// FreeBSD 11 syscalls do the same job (access, readlink; the kernel lacks
// eaccess too, see faccessat). Relative to another directory fd
// there's no reliable way to rebuild the path on FreeBSD 11, so that stays
// ENOSYS rather than guessing.

const AT_FDCWD: c_int = -100;
const AT_EACCESS: c_int = 0x100;
const SYS_ACCESS: u64 = 33;
const SYS_READLINK: u64 = 58;
const SYS_EACCESS: u64 = 376;

unsafe fn at_cwd_or_absolute(fd: c_int, path: *const c_char) -> bool {
    fd == AT_FDCWD || (!path.is_null() && *path == b'/' as c_char)
}

#[no_mangle]
pub unsafe extern "C" fn faccessat(fd: c_int, path: *const c_char, mode: c_int, flag: c_int) -> c_int {
    if !at_cwd_or_absolute(fd, path) || flag & !AT_EACCESS != 0 {
        *__error() = ENOSYS;
        return -1;
    }
    if flag & AT_EACCESS == 0 {
        return syscall6(SYS_ACCESS, [path as u64, mode as u64, 0, 0, 0, 0]) as c_int;
    }
    let ret = syscall6(SYS_EACCESS, [path as u64, mode as u64, 0, 0, 0, 0]) as c_int;
    if ret == 0 || *__error() != ENOSYS {
        return ret;
    }
    // The PS5 kernel lacks eaccess too. access checks the real ids instead
    // of the effective ones, so it gives the same answer exactly when they
    // match; otherwise there's no faithful substitute.
    let ids = |n| syscall6(n, [0; 6]);
    if ids(SYS_GETUID) == ids(SYS_GETEUID) && ids(SYS_GETGID) == ids(SYS_GETEGID) {
        return syscall6(SYS_ACCESS, [path as u64, mode as u64, 0, 0, 0, 0]) as c_int;
    }
    *__error() = ENOSYS;
    -1
}

const SYS_GETUID: u64 = 24;
const SYS_GETEUID: u64 = 25;
const SYS_GETEGID: u64 = 43;
const SYS_GETGID: u64 = 47;

#[no_mangle]
pub unsafe extern "C" fn readlinkat(fd: c_int, path: *const c_char, buf: *mut c_char, size: usize) -> isize {
    if !at_cwd_or_absolute(fd, path) {
        *__error() = ENOSYS;
        return -1;
    }
    syscall6(SYS_READLINK, [path as u64, buf as u64, size as u64, 0, 0, 0]) as isize
}

// ---------------------------------------------------------------------------
// clock_nanosleep: what std::thread::sleep calls on FreeBSD. Unlike other
// syscalls it returns the error number itself, not -1 + errno, so it can't
// use the generic stub. If the kernel lacks it (FreeBSD before 11.1), fall
// back to nanosleep (240), which sleeps on CLOCK_REALTIME's rate: fine for
// relative sleeps; absolute ones are turned into relative ones first.

const SYS_NANOSLEEP: u64 = 240;
const SYS_CLOCK_GETTIME: u64 = 232;
const SYS_CLOCK_NANOSLEEP: u64 = 244;
const TIMER_ABSTIME: c_int = 1;
const EINVAL: c_int = 22;

#[no_mangle]
pub unsafe extern "C" fn clock_nanosleep(clock: c_int, flags: c_int, rq: *const Timespec, rm: *mut Timespec) -> c_int {
    let saved = *__error();
    let ret = syscall6(SYS_CLOCK_NANOSLEEP, [clock as u64, flags as u64, rq as u64, rm as u64, 0, 0]);
    let err = *__error();
    *__error() = saved;
    if ret == 0 {
        return 0;
    }
    if err != ENOSYS {
        return err;
    }
    if rq.is_null() {
        return EINVAL;
    }
    let mut req = *rq;
    if flags & TIMER_ABSTIME != 0 {
        let mut now = Timespec { sec: 0, nsec: 0 };
        if syscall6(SYS_CLOCK_GETTIME, [clock as u64, &mut now as *mut _ as u64, 0, 0, 0, 0]) != 0 {
            let e = *__error();
            *__error() = saved;
            return e;
        }
        req.sec -= now.sec;
        req.nsec -= now.nsec;
        if req.nsec < 0 {
            req.nsec += 1_000_000_000;
            req.sec -= 1;
        }
        if req.sec < 0 {
            return 0;
        }
    }
    // rm is only filled in for relative sleeps, as POSIX says.
    let rm = if flags & TIMER_ABSTIME != 0 { core::ptr::null_mut() } else { rm };
    let ret = syscall6(SYS_NANOSLEEP, [&req as *const _ as u64, rm as u64, 0, 0, 0, 0]);
    let err = *__error();
    *__error() = saved;
    if ret == 0 { 0 } else { err }
}

// ---------------------------------------------------------------------------
// accept: measured on 13.60, a thread asleep inside a blocking accept() when
// the console enters rest mode powers the console off, while a thread asleep
// in kevent on the same socket survives (rest_block / rest_kevent probes).
// So for a blocking socket, wait for a connection in kevent first and only
// then call accept. Non-blocking sockets (tokio, mio) go straight through.
//
// There is a short window between kevent reporting a connection and accept
// taking it; if the client resets in between, accept blocks as before.

const SYS_ACCEPT: u64 = 30;
const SYS_KQUEUE: u64 = 362;
const SYS_CLOSE: u64 = 6;
const EVFILT_READ: i16 = -1;
const EV_ADD: u16 = 0x1;
const EV_ONESHOT: u16 = 0x10;
const EINTR: c_int = 4;

/// Sleeps in kevent until `fd` is readable. Returns -1 with errno only for
/// EINTR (which the caller must report); any other failure returns 0 so the
/// caller falls back to its normal blocking call.
unsafe fn wait_readable(fd: c_int) -> c_int {
    let fl = sys_fcntl(fd, F_GETFL, 0);
    if fl < 0 || fl & O_NONBLOCK != 0 {
        return 0;
    }
    kevent_wait(fd, core::ptr::null())
}

#[no_mangle]
pub unsafe extern "C" fn accept(fd: c_int, addr: *mut c_void, len: *mut u32) -> c_int {
    if wait_readable(fd) < 0 {
        return -1;
    }
    syscall6(SYS_ACCEPT, [fd as u64, addr as u64, len as u64, 0, 0, 0]) as c_int
}

// ---------------------------------------------------------------------------
// recv/recvfrom/recvmsg (std's TcpStream/UdpSocket reads): the same kevent
// wait as accept, for a blocking socket, so no thread sleeps inside the
// receive call itself. SO_RCVTIMEO (std's set_read_timeout) becomes the
// kevent timeout, and runs out with EAGAIN as the real call would. The SDK
// takes these from libkernel_web.so; a static definition here wins.

const SYS_RECVMSG: u64 = 27;
const SYS_RECVFROM: u64 = 29;
const SYS_GETSOCKOPT: u64 = 118;
const SOL_SOCKET: c_int = 0xffff;
const SO_RCVTIMEO: c_int = 0x1006;
const MSG_DONTWAIT: c_int = 0x80;
const EAGAIN: c_int = 35;

#[repr(C)]
struct Timeval {
    sec: i64,
    usec: i64,
}

/// For a blocking socket, sleeps in kevent until it's readable or its
/// SO_RCVTIMEO runs out. -1 + errno (EINTR, or EAGAIN on timeout) means the
/// receive must report that; 0 means go ahead with it.
unsafe fn wait_socket_readable(fd: c_int, flags: c_int) -> c_int {
    if flags & MSG_DONTWAIT != 0 {
        return 0;
    }
    let fl = sys_fcntl(fd, F_GETFL, 0);
    if fl < 0 || fl & O_NONBLOCK != 0 {
        return 0;
    }
    let saved = *__error();
    let mut tv = Timeval { sec: 0, usec: 0 };
    let mut len = core::mem::size_of::<Timeval>() as u32;
    let r = syscall6(SYS_GETSOCKOPT, [
        fd as u64, SOL_SOCKET as u64, SO_RCVTIMEO as u64,
        &mut tv as *mut _ as u64, &mut len as *mut _ as u64, 0,
    ]);
    *__error() = saved;
    if r < 0 {
        return 0; // not a socket: leave it to the real call
    }
    let ts = Timespec { sec: tv.sec, nsec: tv.usec * 1000 };
    let timeout = if tv.sec == 0 && tv.usec == 0 { core::ptr::null() } else { &ts as *const Timespec };
    kevent_wait(fd, timeout)
}

/// Shared by accept and the receives: one-shot EVFILT_READ wait on a
/// private kqueue. Only EINTR and a timeout are reported; other failures
/// return 0 so the caller falls back to its plain blocking call.
unsafe fn kevent_wait(fd: c_int, timeout: *const Timespec) -> c_int {
    let saved = *__error();
    let kq = syscall6(SYS_KQUEUE, [0; 6]);
    if kq < 0 {
        *__error() = saved;
        return 0;
    }
    let ch = Kevent11 {
        ident: fd as usize, filter: EVFILT_READ, flags: EV_ADD | EV_ONESHOT,
        fflags: 0, data: 0, udata: core::ptr::null_mut(),
    };
    let mut ev = ch;
    let r = syscall6(SYS_KEVENT, [kq as u64, &ch as *const _ as u64, 1, &mut ev as *mut _ as u64, 1, timeout as u64]);
    let err = *__error();
    syscall6(SYS_CLOSE, [kq as u64, 0, 0, 0, 0, 0]);
    if r < 0 && err == EINTR {
        *__error() = EINTR;
        return -1;
    }
    if r == 0 && !timeout.is_null() {
        *__error() = EAGAIN;
        return -1;
    }
    *__error() = saved;
    0
}

#[no_mangle]
pub unsafe extern "C" fn recvfrom(
    fd: c_int, buf: *mut c_void, len: usize, flags: c_int, from: *mut c_void, fromlen: *mut u32,
) -> isize {
    if wait_socket_readable(fd, flags) < 0 {
        return -1;
    }
    syscall6(SYS_RECVFROM, [fd as u64, buf as u64, len as u64, flags as u64, from as u64, fromlen as u64]) as isize
}

#[no_mangle]
pub unsafe extern "C" fn recv(fd: c_int, buf: *mut c_void, len: usize, flags: c_int) -> isize {
    recvfrom(fd, buf, len, flags, core::ptr::null_mut(), core::ptr::null_mut())
}

#[no_mangle]
pub unsafe extern "C" fn recvmsg(fd: c_int, msg: *mut c_void, flags: c_int) -> isize {
    if wait_socket_readable(fd, flags) < 0 {
        return -1;
    }
    syscall6(SYS_RECVMSG, [fd as u64, msg as u64, flags as u64, 0, 0, 0]) as isize
}
