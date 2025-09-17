use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());

    let major = env!("CARGO_PKG_VERSION_MAJOR")
        .parse::<u16>()
        .expect("should have major version");

    let minor = env!("CARGO_PKG_VERSION_MINOR")
        .parse::<u16>()
        .expect("should have minor version");

    let patch = env!("CARGO_PKG_VERSION_PATCH")
        .parse::<u32>()
        .expect("should have patch-level version");

    File::create(out.join("version.rs"))
        .unwrap()
        .write_all(
            format!(
                r##"
const VERSION_MAJOR: u16 = {};
const VERSION_MINOR: u16 = {};
const VERSION_PATCH: u32 = {};
"##,
                major, minor, patch
            )
            .as_bytes(),
        )
        .unwrap();

    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
