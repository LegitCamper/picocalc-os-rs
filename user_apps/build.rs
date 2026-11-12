use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest path")?);
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing build output path")?);
    let kernel = env::var("CARGO_PKG_NAME")? == "kernel";
    let memory = if kernel {
        manifest.join("rp2350.x")
    } else {
        manifest
            .parent()
            .ok_or("missing app parent")?
            .join("memory.x")
    };
    fs::copy(&memory, out.join("memory.x"))?;
    println!("cargo:rerun-if-changed={}", memory.display());
    println!("cargo:rustc-link-search={}", out.display());
    if kernel {
        println!("cargo:rustc-link-arg-bins=--nmagic");
        println!("cargo:rustc-link-arg-bins=-Tlink.x");
        println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    } else {
        println!("cargo:rustc-link-arg-bins=-Tmemory.x");
    }
    Ok(())
}
