use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

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

    let build_id = build_id();

    // `{:?}` on a &str emits a properly escaped Rust string literal, quotes
    // included. Git refnames forbid backslash but permit `"`, so a tag like
    // `firmware-v1.0"x` would otherwise emit a syntax error.
    File::create(out.join("version.rs"))
        .unwrap()
        .write_all(
            format!(
                r##"
pub(crate) const VERSION_MAJOR: u16 = {major};
pub(crate) const VERSION_MINOR: u16 = {minor};
pub(crate) const VERSION_PATCH: u32 = {patch};

/// Firmware build identity from `git describe`, or `"unknown"`.
pub(crate) const BUILD_ID: &str = {build_id:?};
"##
            )
            .as_bytes(),
        )
        .unwrap();

    println!("cargo:rerun-if-changed=memory.x");

    // Force this build script to re-run on EVERY build. Cargo treats a
    // nonexistent path as always-changed.
    //
    // This is deliberate and load-bearing. `rerun-if-changed=memory.x` above
    // NARROWS re-runs to that one file, so without this line the embedded
    // BUILD_ID goes stale across incremental builds: after a commit, or after
    // editing a handler, cargo rebuilds the crate but does not re-run this
    // script, and the firmware keeps reporting the previous commit with no
    // `-dirty` marker. A stale build ID is worse than none — it would CONFIRM
    // a wrong conclusion, which is exactly the misidentification this field
    // exists to prevent (issue #159; AGENTS.md §13.17, 2026-08-26).
    //
    // Cost is one `git describe` (~5 ms) per build. Do not "optimise" it away.
    // The path is resolved relative to the package root and MUST NOT exist.
    println!("cargo:rerun-if-changed=.pdg-always-rerun");

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

/// Describe the current firmware build from git.
///
/// Every flag is load-bearing, and each has a *silent* failure mode:
///
/// * `--tags` — the `firmware-v*` tags are a MIX of annotated and lightweight.
///   Without this, `git describe` considers only annotated tags and resolves
///   hundreds of commits too far back (measured: `firmware-v0.10.0-302-g...`).
/// * `--match firmware-v*` — without it, describe picks the nearest tag in ANY
///   namespace (measured: `application-v0.9.0-25-g...`).
/// * `--always` — falls back to a bare hash instead of failing when no matching
///   tag is reachable, e.g. in a shallow CI clone.
/// * `--dirty` — marks a locally modified tree. This is the single most
///   valuable part of the field for a bisecting developer.
///
/// No shell is involved: the arguments are separate argv entries, so
/// `firmware-v*` is passed through literally and is never glob-expanded.
///
/// Returns `"unknown"` when git is unavailable, exits non-zero, or there is no
/// repository. The build always succeeds; a source tarball must still build.
fn build_id() -> String {
    let output = Command::new("git")
        .args(["describe", "--always", "--dirty", "--tags", "--match", "firmware-v*"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();

    let described = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            println!(
                "cargo:warning=`git describe` unavailable; \
                 device/info will report build_id=\"unknown\""
            );
            return "unknown".to_string();
        }
    };

    if described.is_empty() {
        return "unknown".to_string();
    }

    // Truncate on a char boundary. git describe output is ASCII in practice,
    // but a tag name may legally carry UTF-8 and a byte-index slice would
    // panic mid-codepoint. Keep this in sync with
    // `pico_de_gallo_internal::BUILD_ID_CAPACITY`.
    const BUILD_ID_CAPACITY: usize = 64;
    if described.len() <= BUILD_ID_CAPACITY {
        described
    } else {
        let mut end = BUILD_ID_CAPACITY;
        while !described.is_char_boundary(end) {
            end -= 1;
        }
        described[..end].to_string()
    }
}
