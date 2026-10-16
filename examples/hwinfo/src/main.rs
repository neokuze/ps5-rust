#![no_std]
#![no_main]

use ps5::{hw, klog, Toast};

#[no_mangle]
pub extern "C" fn main() -> i32 {
    let model = hw::model_name();
    let model = model.as_ref().map(|s| s.as_str()).unwrap_or("?");
    klog::log(model);

    // Two-digit temperature without core::fmt.
    let mut line = *b"CPU: ??C";
    if let Ok(t) = hw::cpu_temperature() {
        let t = t.clamp(0, 99) as u8;
        line[5] = b'0' + t / 10;
        line[6] = b'0' + t % 10;
    }
    let line = core::str::from_utf8(&line).unwrap_or("");

    match Toast::new(model).sub(line).send() {
        Ok(()) => 0,
        Err(e) => e.0,
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
