fn main() {
    use std::env;
    use std::path::PathBuf;

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = env::var("OUT_DIR").unwrap();
    let header_path = PathBuf::from(&out).join("include").join("pico_de_gallo.h");

    cbindgen::generate(crate_dir)
        .expect("Unable to generate bindings")
        .write_to_file(header_path);

    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=cbindgen.toml");
}
