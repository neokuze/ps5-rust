//! Safe, allocation-free wrappers over the PS5 payload SDK.
//!
//! Anything not wrapped here is still reachable, unsafely, via [`sys`].
//! Each module notes the `-l` flag its functions need at link time.
#![no_std]

mod buf;
pub mod app;
pub mod klog;
pub mod notify;
pub mod payload;
pub mod random;
pub mod system;
pub mod hw;

pub use buf::CString;
pub use notify::{notify, Toast};
/// The raw bindings, for everything without a safe wrapper.
pub use ps5_sys as sys;

/// Raw SDK error code (negative `SCE_*` value or errno-style int).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub i32);

pub type Result<T> = core::result::Result<T, Error>;

pub(crate) fn check(code: i32) -> Result<()> {
    if code == 0 { Ok(()) } else { Err(Error(code)) }
}

/// Firmware version as reported by the kernel, e.g. 0x13600000.
pub fn fw_version() -> u32 {
    unsafe { sys::kernel_get_fw_version() }
}

/// Current process id.
pub fn pid() -> i32 {
    unsafe { sys::getpid() }
}

/// Sleeps the calling thread.
pub fn sleep_us(us: u32) {
    unsafe { sys::usleep(us) };
}
