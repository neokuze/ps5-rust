//! Kernel log (`/dev/klog`), readable with a klog client on port 3232.

use crate::buf::Buf;

/// Writes one line to the kernel log (truncated at 1023 bytes).
pub fn log(line: &str) {
    let s = Buf::<1024>::from_str(line);
    unsafe { ps5_sys::klog_puts(s.as_ptr()) };
}
