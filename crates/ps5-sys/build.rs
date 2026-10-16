use std::{env, path::PathBuf};

fn main() {
    let sdk = env::var("PS5_PAYLOAD_SDK").unwrap_or_else(|_| "/opt/ps5-payload-sdk".into());
    println!("cargo:rerun-if-env-changed=PS5_PAYLOAD_SDK");
    println!("cargo:rerun-if-changed=include");

    // feature -> Sony libs it needs (libkernel_web, libSceLibcInternal and
    // libSceNet are always linked by prospero-clang).
    for (feature, libs) in [
        ("KERNEL_SYS", &["kernel_sys"][..]),
        ("NOTIFICATION", &["SceNotification"]),
        ("RANDOM", &["SceRandom"]),
        ("USER_SERVICE", &["SceUserService"]),
        ("SYSTEM_SERVICE", &["SceSystemService"]),
        ("APP_INST_UTIL", &["SceIpmi", "SceAppInstUtil"]),
        ("SSL", &["SceSsl"]),
        ("HTTP2", &["SceHttp2"]),
    ] {
        if env::var_os(format!("CARGO_FEATURE_{feature}")).is_some() {
            for lib in libs {
                println!("cargo:rustc-link-lib=dylib={lib}");
            }
        }
    }

    let bindings = bindgen::Builder::default()
        .header("include/wrapper.h")
        .clang_args([
            // PS5 is FreeBSD + SysV ABI; the sie-ps5 triple makes bindgen drop functions.
            "--target=x86_64-unknown-freebsd",
            "-nostdinc",
            &format!("-isystem{sdk}/target/include"),
            "-Iinclude",
        ])
        .use_core()
        .ctypes_prefix("core::ffi")
        .layout_tests(false)
        // Bind whole files rather than name patterns: every function, type
        // and constant declared in these headers (plus what they depend on).
        .allowlist_file(concat!(
            r".*/ps5/[a-z]+\.h|.*sce_extra\.h|",
            r".*/(dirent|dlfcn|errno|fcntl|pthread|stdio|stdlib|string|time|unistd)\.h|",
            r".*/arpa/inet\.h|.*/netinet/in\.h|.*/sys/(mman|socket|stat|sysctl)\.h",
        ))
        // core already provides these; bindgen would type size_t as u64.
        .blocklist_function("mem(cmp|cpy|move|set)|strlen")
        .blocklist_item("FP_.*") // libm-style float-class macros collide
        .default_enum_style(bindgen::EnumVariation::ModuleConsts)
        .generate()
        .expect("bindgen failed; is libclang installed and PS5_PAYLOAD_SDK set?");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs");
    bindings.write_to_file(out).unwrap();
}
