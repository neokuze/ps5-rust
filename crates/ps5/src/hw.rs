//! Hardware info and sensors. Feature `hw`.

use crate::{check, CString, Result};

/// Console model, e.g. "CFI-1216A".
pub fn model_name() -> Result<CString<1000>> {
    let mut s = CString::zeroed();
    check(unsafe { ps5_sys::sceKernelGetHwModelName(s.as_mut_ptr()) })?;
    Ok(s)
}

pub fn serial_number() -> Result<CString<1000>> {
    let mut s = CString::zeroed();
    check(unsafe { ps5_sys::sceKernelGetHwSerialNumber(s.as_mut_ptr()) })?;
    Ok(s)
}

/// CPU frequency in Hz.
pub fn cpu_frequency() -> i64 {
    unsafe { ps5_sys::sceKernelGetCpuFrequency() as i64 }
}

/// CPU temperature in °C.
pub fn cpu_temperature() -> Result<i32> {
    let mut t = 0;
    check(unsafe { ps5_sys::sceKernelGetCpuTemperature(&mut t) })?;
    Ok(t)
}

/// SoC sensor temperature in °C (sensor 0 is the main one).
pub fn soc_temperature(sensor: i32) -> Result<i32> {
    let mut t = 0;
    check(unsafe { ps5_sys::sceKernelGetSocSensorTemperature(sensor, &mut t) })?;
    Ok(t)
}
