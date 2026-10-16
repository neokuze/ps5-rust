//! 1. Rust's FreeBSD std asks the linker for libraries the PS5 SDK doesn't
//!    ship (-lexecinfo, -lkvm, -lrt, ...). Nothing in them is needed, so we
//!    hand the linker empty archives with those names.
//!
//! 2. A patched copy of the SDK's libc.a, made here from the installed SDK
//!    (no SDK binaries are stored in this repo) and bundled into this rlib,
//!    which the linker sees before the SDK's own libc.a:
//!    - `fstatat` -> `__sdk_fstatat`, freeing the name for the FreeBSD 12
//!      `fstatat` in src/lib.rs (the original would clash with it);
//!    - mktemp.o's `stat`/`lstat` -> `__ps5_stat11`/`__ps5_lstat11`, which
//!      keep the FreeBSD 11 layout that code was compiled for (the plain
//!      names are now the FreeBSD 12 wrappers in src/lib.rs).
//!    Nothing else in libc.a defines or references these names (checked with
//!    llvm-nm), so the renames only touch those spots.
use std::{env, fs, path::PathBuf, process::Command};

const LIBS: &[&str] = &["execinfo", "gcc_s", "m", "rt", "util", "kvm", "memstat", "procstat", "devstat"];

const RENAMES: &[(&str, &str)] = &[
    ("fstatat", "__sdk_fstatat"),
    ("stat", "__ps5_stat11"),
    ("lstat", "__ps5_lstat11"),
];

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    for lib in LIBS {
        // An archive with no members: just the magic header.
        fs::write(out.join(format!("lib{lib}.a")), b"!<arch>\n").unwrap();
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PS5_PAYLOAD_SDK");

    let sdk = PathBuf::from(env::var("PS5_PAYLOAD_SDK").unwrap_or_else(|_| "/opt/ps5-payload-sdk".into()));
    let libc = sdk.join("target/lib/libc.a");
    println!("cargo:rerun-if-changed={}", libc.display());

    let patched = out.join("libps5c.a");
    let mut cmd = Command::new(sdk.join("bin/llvm-objcopy"));
    for (from, to) in RENAMES {
        cmd.arg(format!("--redefine-sym={from}={to}"));
    }
    let status = cmd
        .arg(&libc)
        .arg(&patched)
        .status()
        .unwrap_or_else(|e| panic!("running llvm-objcopy from {}: {e}", sdk.display()));
    assert!(status.success(), "llvm-objcopy failed on {}", libc.display());

    // Bundled into the rlib, so it comes before std's `-lc` on the link line.
    println!("cargo:rustc-link-lib=static=ps5c");
}
