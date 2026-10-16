//! The payload's own lifecycle.

pub use ps5_sys::payload_args_t as Args;

/// Arguments the loader passed to `_start()`.
pub fn args() -> &'static Args {
    // The SDK keeps these in a static that lives for the whole payload.
    unsafe { &*ps5_sys::payload_get_args() }
}

/// Exits the payload. In a hijacked process, that process resumes.
pub fn exit(code: i32) -> ! {
    unsafe { ps5_sys::payload_exit(code) };
    loop {}
}
