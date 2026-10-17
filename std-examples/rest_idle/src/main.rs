//! Rest-mode isolation test: the smallest resident payload. No sockets of
//! its own, no extra threads; it only sleeps in a loop and compares clocks.
//! If the console survives rest mode with this running, the crash seen with
//! rest_probe comes from what that probe adds (listening socket, thread).
//! Shows a toast on a clock gap over 10 s, then keeps going; exits after 15
//! minutes so it doesn't stay resident.
use ps5::Toast;
use ps5_std_rt as _;
use std::ffi::c_int;
use std::time::Duration;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(clock: c_int, ts: *mut Timespec) -> c_int;
}

fn now(clock: c_int) -> i64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(clock, &mut ts) };
    ts.sec
}

fn toast(msg: &str) {
    let _ = Toast::new(msg).sub("rest_idle").send();
}

fn main() {
    toast("idle; put the console in rest now");
    let start = now(4);
    let (mut real, mut mono) = (now(0), now(4));
    while now(4) - start < 15 * 60 {
        std::thread::sleep(Duration::from_secs(1));
        let (r, m) = (now(0), now(4));
        if r - real > 10 || m - mono > 10 {
            toast(&format!("survived rest: real +{}s mono +{}s", r - real, m - mono));
        }
        (real, mono) = (r, m);
    }
    toast("15 min, exiting");
}
