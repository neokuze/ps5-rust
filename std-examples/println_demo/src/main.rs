//! println!/eprintln! from a payload. The ELF loader hands the payload the
//! connection it was sent over as stdin/stdout/stderr, and prospero-deploy
//! (`make std-test`) keeps reading it, so these lines show up in the terminal
//! that sent the payload:
//!   make std-test STD_EXAMPLE=println_demo
//! Prints for about 10 seconds, then exits. Also tries a read from stdin, which
//! is the same connection (`make std-run` keeps your keyboard on it).
use ps5::Toast;
use ps5_std_rt as _;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

fn main() {
    let _ = Toast::new("printing to the sender's terminal").sub("println_demo").send();
    let start = Instant::now();
    println!("hello from the PS5: println! goes back over the loader connection");
    eprintln!("and eprintln! too (stderr is the same socket)");
    for i in 1..=5 {
        std::thread::sleep(Duration::from_secs(2));
        println!("tick {i}/5 at {:.1} s", start.elapsed().as_secs_f64());
    }

    print!("type a line and press Enter (20 s): ");
    let _ = std::io::stdout().flush();
    // stdin is a blocking socket: wait on it in a thread so main can give up.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        let _ = tx.send(line);
    });
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(line) if !line.trim().is_empty() => println!("\ngot from your keyboard: {:?}", line.trim()),
        _ => println!("\n(no input)"),
    }
    println!("bye after {:.1} s", start.elapsed().as_secs_f64());
}
