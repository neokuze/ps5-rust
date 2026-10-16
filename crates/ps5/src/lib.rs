//! Safe wrappers over the PS5 payload SDK. Allocation-free unless the
//! `alloc` feature is on (heap + threads).
//!
//! Anything not wrapped here is still reachable, unsafely, via [`sys`].
//! Modules behind Cargo features (`notify`, `hw`, `http`, ...) link the
//! Sony libraries they need; `full` enables all of them.
#![no_std]

mod buf;
pub mod fmt;
pub mod klog;
pub mod net;
pub mod payload;

#[cfg(feature = "alloc")] extern crate alloc;
#[cfg(feature = "alloc")] pub mod heap;
#[cfg(feature = "alloc")] pub mod thread;
#[cfg(feature = "app")] pub mod app;
#[cfg(feature = "http")] pub mod http;
#[cfg(feature = "hw")] pub mod hw;
#[cfg(feature = "notify")] pub mod notify;
#[cfg(feature = "random")] pub mod random;
#[cfg(feature = "system")] pub mod system;

pub use buf::CString;
#[cfg(feature = "notify")]
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
