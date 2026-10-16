//! Threads over pthreads (libkernel). Needs a global allocator, see
//! [`crate::heap`].
//!
//! ```ignore
//! let h = ps5::thread::spawn(|| 2 + 2)?;
//! assert_eq!(h.join(), 4);
//! ```

use crate::{Error, Result};
use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ops::{Deref, DerefMut};
use ps5_sys as sys;

/// Owns a running thread; `join` waits and returns its result.
/// Dropping it without joining detaches the thread.
pub struct JoinHandle<T> {
    id: sys::pthread_t,
    result: *mut Option<T>,
}

unsafe impl<T: Send> Send for JoinHandle<T> {}

struct Start<F, T> {
    f: F,
    result: *mut Option<T>,
}

/// Starts `f` on a new thread.
pub fn spawn<F, T>(f: F) -> Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    extern "C" fn entry<F: FnOnce() -> T, T>(arg: *mut c_void) -> *mut c_void {
        let start = unsafe { Box::from_raw(arg as *mut Start<F, T>) };
        let value = (start.f)();
        unsafe { *start.result = Some(value) };
        core::ptr::null_mut()
    }

    let result = Box::into_raw(Box::new(None));
    let start = Box::into_raw(Box::new(Start { f, result }));
    let mut id: sys::pthread_t = unsafe { core::mem::zeroed() };
    let rc = unsafe { sys::pthread_create(&mut id, core::ptr::null(), Some(entry::<F, T>), start.cast()) };
    if rc != 0 {
        unsafe {
            drop(Box::from_raw(start));
            drop(Box::from_raw(result));
        }
        return Err(Error(rc));
    }
    Ok(JoinHandle { id, result })
}

impl<T> JoinHandle<T> {
    /// Waits for the thread to finish and returns what it returned.
    pub fn join(self) -> Result<T> {
        let me = core::mem::ManuallyDrop::new(self);
        let rc = unsafe { sys::pthread_join(me.id, core::ptr::null_mut()) };
        let result = unsafe { Box::from_raw(me.result) };
        if rc != 0 {
            core::mem::forget(result); // the thread may still write it
            return Err(Error(rc));
        }
        // With panic=abort the thread can't return without a value.
        result.ok_or(Error(-1))
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        // Detached: the result box leaks, since the thread may still write it.
        unsafe { sys::pthread_detach(self.id) };
    }
}

/// A mutex over `pthread_mutex_t`.
pub struct Mutex<T> {
    raw: UnsafeCell<sys::pthread_mutex_t>,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub fn new(value: T) -> Result<Self> {
        // pthread_mutex_t is a pointer on FreeBSD, so the Mutex can move.
        let mut raw: sys::pthread_mutex_t = unsafe { core::mem::zeroed() };
        crate::check(unsafe { sys::pthread_mutex_init(&mut raw, core::ptr::null()) })?;
        Ok(Mutex { raw: UnsafeCell::new(raw), data: UnsafeCell::new(value) })
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        unsafe { sys::pthread_mutex_lock(self.raw.get()) };
        MutexGuard { m: self }
    }
}

impl<T> Drop for Mutex<T> {
    fn drop(&mut self) {
        unsafe { sys::pthread_mutex_destroy(self.raw.get()) };
    }
}

pub struct MutexGuard<'a, T> {
    m: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.m.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.m.data.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { sys::pthread_mutex_unlock(self.m.raw.get()) };
    }
}
