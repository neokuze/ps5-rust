//! Heap + threads: 4 threads each fill a Vec of 1M numbers on the heap,
//! sum it, and add the sum to a total shared through Arc<Mutex<_>>.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use ps5::thread::{self, Mutex};
use ps5::{log, Toast};

const THREADS: u64 = 4;
const N: u64 = 1_000_000;

#[no_mangle]
pub extern "C" fn main() -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => {
            log!("threads failed: ", e.0);
            let _ = Toast::new("threads failed").sub(ps5::line!("code ", e.0).as_str()).send();
            e.0
        }
    }
}

fn run() -> ps5::Result<()> {
    let total = Arc::new(Mutex::new(0u64)?);

    let mut handles = Vec::new();
    for t in 0..THREADS {
        let total = total.clone();
        handles.push(thread::spawn(move || {
            // Thread t sums t*N .. (t+1)*N - 1.
            let nums: Vec<u64> = (t * N..(t + 1) * N).collect();
            let sum: u64 = nums.iter().sum();
            *total.lock() += sum;
            log!("thread ", t, " sum ", sum);
            sum
        })?);
    }

    let mut joined = 0u64;
    for h in handles {
        joined += h.join()?;
    }

    // Sum of 0 .. THREADS*N - 1.
    let m = THREADS * N;
    let expected = m * (m - 1) / 2;
    let total = *total.lock();
    let ok = total == expected && joined == expected;
    log!("threads total ", total, " expected ", expected);

    Toast::new(if ok { "threads OK" } else { "threads MISMATCH" })
        .sub(ps5::line!(THREADS, " threads, total ", total).as_str())
        .send()
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
