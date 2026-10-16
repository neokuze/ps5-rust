#![no_std]
#![no_main]

use ps5::fmt::Hex;
use ps5::http::Http;
use ps5::{hw, log, print, Toast};

/// Set at build time: PS5_TEST_SERVER=http://<pc-ip>:8080 make PAYLOAD=http_test
const SERVER: &str = match option_env!("PS5_TEST_SERVER") {
    Some(s) => s,
    None => "http://192.168.1.136:8080",
};

#[no_mangle]
pub extern "C" fn main() -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => {
            log!("http_test failed: ", e.0, " (", Hex(e.0 as u32 as u64), ")");
            let _ = Toast::new("http_test failed").sub(ps5::line!("error ", Hex(e.0 as u32 as u64)).as_str()).send();
            e.0
        }
    }
}

fn run() -> ps5::Result<()> {
    let http = Http::init("ps5-rust-http_test/0.1")?;

    // 1. GET a greeting from the PC and show it.
    let url = ps5::line!(SERVER, "/hello");
    let mut res = http.get(url.as_str())?;
    let mut body = [0u8; 256];
    let n = res.read_to_fill(&mut body)?;
    let text = core::str::from_utf8(&body[..n]).unwrap_or("<not utf-8>");
    log!("GET /hello -> ", res.status(), " ", text);
    print!("GET /hello -> ", res.status(), " ", text);
    drop(res);

    // 2. POST hardware info back to the PC.
    let model = hw::model_name()?;
    let temp = hw::cpu_temperature().unwrap_or(-1);
    let report = ps5::line!("model=", model, "\ncpu_temp=", temp, "C\nfw=", Hex(ps5::fw_version() as u64));
    let url = ps5::line!(SERVER, "/report");
    let res = http.post(url.as_str(), report.as_bytes())?;
    log!("POST /report -> ", res.status());

    Toast::new(text).sub(ps5::line!("POST /report: ", res.status()).as_str()).send()
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
