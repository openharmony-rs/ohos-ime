//! Host tests link a test binary and therefore need `-lohinputmethod`.
//! Device / OHOS builds use the real system library; this stub is host-only.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("ohos") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let stub_c = out_dir.join("ohinputmethod_stub.c");
    let stub_o = out_dir.join("ohinputmethod_stub.o");
    let stub_a = out_dir.join("libohinputmethod.a");

    fs::write(&stub_c, "void ohos_ime_host_link_stub(void) {}\n").expect("write stub source");

    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&cc)
        .args(["-c", "-o"])
        .arg(&stub_o)
        .arg(&stub_c)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {cc}: {e}"));
    assert!(status.success(), "{cc} failed to compile IME host stub");

    let status = Command::new("ar")
        .args(["crus"])
        .arg(&stub_a)
        .arg(&stub_o)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn ar: {e}"));
    assert!(status.success(), "ar failed to create IME host stub");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rerun-if-changed=build.rs");
}
