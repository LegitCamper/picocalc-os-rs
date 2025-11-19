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
    bindgen();

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
}

fn bindgen() {
    let bindings = bindgen::Builder::default()
        .header("Peanut-GB/peanut_gb.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .disable_nested_struct_naming()
        .clang_arg("-I../../picolibc/newlib/libc/include/")
        .clang_arg("-I../../picolibc/build/")
        .use_core()
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    cc::Build::new()
        .define("PEANUT_GB_IS_LITTLE_ENDIAN", None)
        .define("ENABLE_LCD", None)
        .file("peanut_gb_stub.c")
        .include("Peanut-GB")
        // optimization flags
        .flag("-Ofast")
        .flag("-fdata-sections")
        .flag("-ffunction-sections")
        .flag("-mcpu=cortex-m33")
        .flag("-mthumb")
        .flag("-g0")
        .compile("peanut_gb");

    println!("cargo:rustc-link-search=Peanut-GB");
    println!("cargo:rustc-link-lib=peanut_gb");
}
