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
//! - `fcntl`, emulating `F_DUPFD_CLOEXEC` (rejected by the PS5 kernel).
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
