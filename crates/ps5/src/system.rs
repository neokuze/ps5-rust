//! User/system services. Feature `system`.

use crate::{buf::Buf, check, Result};

/// Keeps the user service initialized; terminates it on drop.
pub struct UserService(());

impl UserService {
    pub fn init() -> Result<Self> {
        check(unsafe { ps5_sys::sceUserServiceInitialize(core::ptr::null_mut()) })?;
        Ok(Self(()))
    }

    /// Opens the system web browser. Needs the user service running.
    pub fn launch_web_browser(&self, url: &str) -> Result<()> {
        let url = Buf::<2048>::from_str(url);
        check(unsafe { ps5_sys::sceSystemServiceLaunchWebBrowser(url.as_ptr(), core::ptr::null_mut()) })
    }
}

impl Drop for UserService {
    fn drop(&mut self) {
        unsafe { ps5_sys::sceUserServiceTerminate() };
    }
}
