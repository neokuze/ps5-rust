#![no_std]
#![no_main]

use ps5::Toast;

#[no_mangle]
pub extern "C" fn main() -> i32 {
    let toast = Toast::new("Hello World from Rust!").sub("via ps5-sys + bindgen");
    match toast.send() {
        Ok(()) => 0,
        Err(e) => e.0,
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
