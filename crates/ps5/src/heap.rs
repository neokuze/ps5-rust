//! Heap allocator over the SDK's libc `malloc`, for `alloc` (`Vec`, `Box`,
//! `String`, `Arc`, ...). The `alloc` feature registers it as the global
//! allocator, so a payload only needs `extern crate alloc;`.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use ps5_sys as sys;

/// malloc guarantees this alignment on amd64 FreeBSD.
const MIN_ALIGN: usize = 16;

pub struct Malloc;

#[global_allocator]
static HEAP: Malloc = Malloc;

unsafe impl GlobalAlloc for Malloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= MIN_ALIGN && layout.align() <= layout.size() {
            sys::malloc(layout.size() as _).cast()
        } else {
            aligned(layout)
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= MIN_ALIGN && layout.align() <= layout.size() {
            sys::calloc(layout.size() as _, 1).cast()
        } else {
            let p = aligned(layout);
            if !p.is_null() {
                ptr::write_bytes(p, 0, layout.size());
            }
            p
        }
    }

    unsafe fn dealloc(&self, p: *mut u8, _: Layout) {
        sys::free(p.cast());
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= MIN_ALIGN && layout.align() <= new_size {
            sys::realloc(p.cast(), new_size as _).cast()
        } else {
            let new = aligned(Layout::from_size_align_unchecked(new_size, layout.align()));
            if !new.is_null() {
                ptr::copy_nonoverlapping(p, new, layout.size().min(new_size));
                sys::free(p.cast());
            }
            new
        }
    }
}

unsafe fn aligned(layout: Layout) -> *mut u8 {
    let mut p = ptr::null_mut();
    let align = layout.align().max(size_of::<usize>());
    if sys::posix_memalign(&mut p, align as _, layout.size() as _) == 0 { p.cast() } else { ptr::null_mut() }
}
