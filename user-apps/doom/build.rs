//! This build script copies the `memory.x` file from the crate root into
//! a directory where the linker can always find it at build time.
//! For many projects this is optional, as the linker always searches the
//! project root directory -- wherever `Cargo.toml` is. However, if you
//! are using a workspace or have a more complicated build setup, this
//! build script becomes required. Additionally, by requesting that
//! Cargo re-run the build script whenever `memory.x` is changed,
//! updating `memory.x` ensures a rebuild of the application with the
//! new memory settings.

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // Put `memory.x` in our output directory and ensure it's
    // on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("../memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());

    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rustc-link-arg-bins=-Tmemory.x");

    let ref dg_src_dir = std::path::PathBuf::from("doomgeneric/doomgeneric");
    let mut dg_c_paths = vec![];
    let mut dg_h_paths = vec![];

    // Find most c and h files
    for entry in std::fs::read_dir(dg_src_dir).unwrap() {
        let entry = entry.unwrap();
        if let Some(filename) = entry.file_name().to_str() {
            if filename.starts_with("doomgeneric")
                || filename.contains("_allegro")
                || filename.contains("_sdl")
                || filename == "i_main.c"
            {
                continue;
            }

            if filename.ends_with(".h") {
                dg_h_paths.push(dg_src_dir.join(filename));
            } else if filename.ends_with(".c") {
                dg_c_paths.push(dg_src_dir.join(filename));
            }
        }
    }
    dg_c_paths
        .iter()
        .chain(dg_h_paths.iter())
        .for_each(|path| println!("cargo:rerun-if-changed={}", path.to_str().unwrap()));

    cc::Build::new()
        .flag("-w") // silence warnings
        .flag("-Os") // optimize for size
        .define("CMAP256", None)
        .define("DOOMGENERIC_RESX", Some("320"))
        .define("DOOMGENERIC_RESY", Some("200"))
        .define("MAXPLAYERS", Some("1"))
        .flag_if_supported("-std=gnu89") // old-style C, allows implicit int
        .flag("-Wno-implicit-function-declaration") // ignore missing prototypes
        .define("_POSIX_C_SOURCE", Some("200809L"))
        .files(dg_c_paths)
        .include(".")
        .compile("doomgeneric");
}
