//! 1. Rust's FreeBSD std asks the linker for libraries the PS5 SDK doesn't
//!    ship (-lexecinfo, -lkvm, -lrt, ...). Nothing in them is needed, so we
//!    hand the linker empty archives with those names.
//!
//! 2. A patched copy of the SDK's libc.a, made here from the installed SDK
//!    (no SDK binaries are stored in this repo) and bundled into this rlib,
//!    which the linker sees before the SDK's own libc.a:
//!    - mktemp.o's `stat`/`lstat` -> `__ps5_stat11`/`__ps5_lstat11`, which
//!      keep the FreeBSD 11 layout that code was compiled for (the plain
//!      names are now the FreeBSD 12 wrappers in src/lib.rs). Nothing else
//!      in libc.a defines or references these names (checked with llvm-nm),
//!      so the rename only touches mktemp.o.
//!    - syscalls.o removed: part 3 regenerates every stub in it (with error
//!      checking; `fstatat` is in src/lib.rs), so neither this copy's nor the
//!      SDK's syscalls.o is ever pulled in.
use std::{env, fs, path::PathBuf, process::Command};

const LIBS: &[&str] = &["execinfo", "gcc_s", "m", "rt", "util", "kvm", "memstat", "procstat", "devstat"];

const RENAMES: &[(&str, &str)] = &[
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

    let objcopy = sdk.join("bin/llvm-objcopy");
    assert!(
        objcopy.exists(),
        "PS5 Payload SDK not found at {} (no bin/llvm-objcopy): set PS5_PAYLOAD_SDK to the SDK's install directory",
        sdk.display()
    );
    // The SDK tools run in a scratch dir under the system temp dir, and only
    // plain file contents are written to OUT_DIR. On a WSL /mnt/c checkout,
    // OUT_DIR is a Windows drive where a normal user can create files but not
    // chmod/chown them, which fs::copy, objcopy and `ar x` all try to do.
    let work = env::temp_dir().join(format!("ps5-std-rt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap_or_else(|e| panic!("creating {}: {e}", work.display()));
    let patched = work.join("libps5c.a");
    let bytes = fs::read(&libc).unwrap_or_else(|e| panic!("reading {}: {e}", libc.display()));
    fs::write(&patched, bytes).unwrap();
    let mut cmd = Command::new(objcopy);
    for (from, to) in RENAMES {
        cmd.arg(format!("--redefine-sym={from}={to}"));
    }
    let status = cmd
        .arg(&patched)
        .status()
        .unwrap_or_else(|e| panic!("running llvm-objcopy from {}: {e}", sdk.display()));
    assert!(status.success(), "llvm-objcopy failed on {}", libc.display());
    let status = Command::new(sdk.join("bin/llvm-ar"))
        .args(["d"])
        .arg(&patched)
        .arg("syscalls.o")
        .status()
        .expect("running llvm-ar");
    assert!(status.success(), "removing syscalls.o from the libc copy");
    fs::write(out.join("libps5c.a"), fs::read(&patched).unwrap()).unwrap();

    // Bundled into the rlib, so it comes before std's `-lc` on the link line.
    println!("cargo:rustc-link-lib=static=ps5c");

    gen_syscall_stubs(&sdk, &work, &out);
    let _ = fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// 3. Error-checking syscall stubs.
//
// The SDK's libc.a syscalls.o has ~230 stubs of the form
//     mov $N, %rax; mov %rcx, %r10; syscall; ret
// with no error check: on failure FreeBSD sets the carry flag and leaves the
// errno value in rax, so these return e.g. 2 instead of -1 / errno = ENOENT.
// std links some of them (dup, openat, unlinkat, ...). We generate the same
// set with the usual FreeBSD epilogue (`jb` to a routine that stores errno
// and returns -1) as global asm in this crate, which the linker prefers over
// the archive. Numbers come from the installed SDK; any stub that doesn't
// have the expected shape stops the build instead of being guessed.

/// Stubs src/lib.rs defines itself.
const STUB_SKIP: &[&str] = &["fstatat", "faccessat", "readlinkat", "clock_nanosleep", "accept"];

pub fn gen_syscall_stubs(sdk: &std::path::Path, work: &std::path::Path, out: &std::path::Path) {
    let status = Command::new(sdk.join("bin/llvm-ar"))
        .current_dir(work)
        .arg("x")
        .arg(sdk.join("target/lib/libc.a"))
        .arg("syscalls.o")
        .status()
        .expect("running llvm-ar");
    assert!(status.success(), "extracting syscalls.o from the SDK libc.a");
    // The SDK's bin/ has no llvm-objdump; its LLVM install does.
    let bindir = Command::new(sdk.join("bin/prospero-llvm-config"))
        .arg("--bindir")
        .output()
        .expect("running prospero-llvm-config");
    let bindir = String::from_utf8(bindir.stdout).unwrap().trim().to_owned();
    let dump = Command::new(std::path::Path::new(&bindir).join("llvm-objdump"))
        .args(["-d", "--no-show-raw-insn", "--no-leading-addr"])
        .arg(work.join("syscalls.o"))
        .output()
        .expect("running llvm-objdump");
    assert!(dump.status.success(), "disassembling syscalls.o");
    let text = String::from_utf8(dump.stdout).unwrap();

    // Collect (name, instructions) per function.
    let mut funcs: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix('<').and_then(|l| l.strip_suffix(">:")) {
            funcs.push((name.to_owned(), Vec::new()));
        } else if let (Some(f), true) = (funcs.last_mut(), line.starts_with(' ')) {
            let ins = line.split('#').next().unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
            if !ins.is_empty() {
                f.1.push(ins);
            }
        }
    }

    const GENERIC: &[&str] = &[
        "movq %rdi, %rax", "movq %rsi, %rdi", "movq %rdx, %rsi", "movq %rcx, %rdx",
        "movq %r8, %r10", "movq %r9, %r8", "movq 0x8(%rsp), %r9", "syscall", "retq", "retq",
    ];
    let mut asm = String::from(
        "# Generated by ps5-std-rt/build.rs from the SDK's syscalls.o.\n\
         .section .text.__ps5_cerror,\"ax\",@progbits\n\
         .p2align 4\n\
         .hidden __ps5_cerror\n\
         __ps5_cerror:\n\
         \tpushq %rax\n\
         \tcallq *__error@GOTPCREL(%rip)\n\
         \tpopq %rcx\n\
         \tmovl %ecx, (%rax)\n\
         \tmovq $-1, %rax\n\
         \tmovq $-1, %rdx\n\
         \tretq\n",
    );
    let mut count = 0;
    for (name, ins) in &funcs {
        if STUB_SKIP.contains(&name.as_str()) {
            continue;
        }
        let body = if ins.iter().map(String::as_str).eq(GENERIC.iter().copied()) {
            GENERIC[..8].iter().map(|i| format!("\t{i}\n")).collect::<String>()
        } else {
            let n = match ins.as_slice() {
                [mov, r10, sc, r1, r2]
                    if r10 == "movq %rcx, %r10" && sc == "syscall" && r1 == "retq" && r2 == "retq" =>
                {
                    mov.strip_prefix("movq $")
                        .and_then(|v| v.strip_suffix(", %rax"))
                        .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), if v.starts_with("0x") { 16 } else { 10 }).ok())
                }
                _ => None,
            };
            let n = n.unwrap_or_else(|| panic!("unexpected SDK syscall stub `{name}`: {ins:?}"));
            format!("\tmovq ${n}, %rax\n\tmovq %rcx, %r10\n\tsyscall\n")
        };
        asm += &format!(
            ".section .text.__ps5_sys.{name},\"ax\",@progbits\n\
             .p2align 4\n\
             .globl {name}\n\
             .type {name},@function\n\
             {name}:\n\
             {body}\
             \tjb __ps5_cerror\n\
             \tretq\n\
             .size {name}, .-{name}\n"
        );
        count += 1;
    }
    assert!(count > 200, "only {count} syscall stubs found in the SDK's syscalls.o");
    fs::write(out.join("syscalls.s"), asm).unwrap();
}
