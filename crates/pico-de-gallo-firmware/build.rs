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
pub(crate) const VERSION_MAJOR: u16 = {};
pub(crate) const VERSION_MINOR: u16 = {};
pub(crate) const VERSION_PATCH: u32 = {};
"##,
                major, minor, patch
            )
            .as_bytes(),
        )
        .unwrap();

    println!("cargo:rerun-if-changed=memory.x");

    // Cargo features cannot carry `#[deprecated]`, so a build-script warning is
    // the only signal available at compile time. Removal is not before
    // 2031-09-01; see crates/pico-de-gallo-firmware/CHANGELOG.md.
    if env::var_os("CARGO_FEATURE_HW_REV1").is_some() {
        println!(
            "cargo:warning=feature `hw-rev1` is deprecated (v1.0 landing board). \
             UART, ADC and 1-Wire endpoints return `Unsupported`. It will not be \
             removed before 2031-09-01; build without `--no-default-features \
             --features hw-rev1` to get the default `hw-rev2` image."
        );
    }

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
