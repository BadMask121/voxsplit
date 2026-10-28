use std::process::Command;

fn main() {
    // ONNX Runtime's Core ML provider uses `@available`, which lowers to
    // `__isPlatformVersionAtLeast` from clang's compiler runtime. rustc links
    // with -nodefaultlibs, so add libclang_rt.osx.a explicitly.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = Command::new("xcrun")
            .args(["clang", "--print-runtime-dir"])
            .output()
            .expect("xcrun not found: install the Xcode command-line tools");
        let dir = String::from_utf8(out.stdout).expect("utf-8 path");
        println!("cargo:rustc-link-search=native={}", dir.trim());
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }
}
