fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if os == "macos" && arch == "aarch64" && std::env::var_os("CARGO_FEATURE_STT").is_some() {
        link_clang_rt_osx();
    }
    if os == "android" && std::env::var_os("CARGO_FEATURE_STT").is_some() {
        stub_pthread();
    }
    tauri_build::build()
}

/// ggml-metal guards newer Metal APIs with `@available`. Built for an older
/// deployment target than the SDK (macOS 11 here), clang lowers those checks
/// to `__isPlatformVersionAtLeast` from `libclang_rt.osx.a`, which rustc does
/// not link (`-nodefaultlibs`). Same fix as Handy.
fn link_clang_rt_osx() {
    let out = std::process::Command::new("clang")
        .arg("--print-resource-dir")
        .output()
        .expect("run `clang --print-resource-dir`");
    let dir = String::from_utf8(out.stdout).expect("utf-8");
    let darwin = std::path::Path::new(dir.trim()).join("lib/darwin");
    assert!(darwin.join("libclang_rt.osx.a").exists(), "libclang_rt.osx.a not found in {}", darwin.display());
    println!("cargo:rustc-link-search=native={}", darwin.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}

/// transcribe.cpp's CMake asks for `-lpthread`. On Android pthreads live in
/// bionic's libc and there is no libpthread, so give the linker an empty one.
fn stub_pthread() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("pthread-stub");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("libpthread.a"), b"!<arch>\n").unwrap();
    println!("cargo:rustc-link-search=native={}", out.display());
}
