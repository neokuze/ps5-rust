//! Running-app info and app install. Feature `app`.

use crate::{buf::Buf, check, CString, Result};

pub struct AppInfo {
    pub app_id: u32,
    pub app_type: u32,
    /// e.g. "PPSA01234"; empty for non-app processes.
    pub title_id: CString<10>,
}

/// Info about the app a process belongs to.
pub fn info(pid: i32) -> Result<AppInfo> {
    let mut raw: ps5_sys::app_info_t = unsafe { core::mem::zeroed() };
    check(unsafe { ps5_sys::sceKernelGetAppInfo(pid, &mut raw) })?;
    Ok(AppInfo {
        app_id: raw.app_id,
        app_type: raw.app_type,
        title_id: CString(raw.title_id.map(|c| c as u8)),
    })
}

/// Installs an extracted title directory, e.g.
/// `install_title_dir("PPSA01234", "/data/homebrew/")`. Back up first:
/// on firmwares where PSN won't re-download, a failed install can cost data.
pub fn install_title_dir(title_id: &str, dir: &str) -> Result<()> {
    let id = Buf::<16>::from_str(title_id);
    let dir = Buf::<1024>::from_str(dir);
    unsafe {
        check(ps5_sys::sceAppInstUtilInitialize())?;
        check(ps5_sys::sceAppInstUtilAppInstallTitleDir(id.as_ptr(), dir.as_ptr(), core::ptr::null_mut()))
    }
}
