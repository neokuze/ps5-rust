//! Rust's FreeBSD std asks the linker for libraries the PS5 SDK doesn't ship
//! (-lexecinfo, -lkvm, -lrt, ...). Nothing in them is actually needed, so we
//! hand the linker empty archives with those names.
use std::{env, fs, path::PathBuf};

const LIBS: &[&str] = &["execinfo", "gcc_s", "m", "rt", "util", "kvm", "memstat", "procstat", "devstat"];

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    for lib in LIBS {
        // An archive with no members: just the magic header.
        fs::write(out.join(format!("lib{lib}.a")), b"!<arch>\n").unwrap();
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
}
