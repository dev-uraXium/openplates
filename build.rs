//! Copies the private chart bundle + key into OUT_DIR.
//! If `private/` is missing (e.g. a fresh clone of the public repo), the app is
//! built with an empty placeholder bundle and shows a notice on start-up.

use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bin = PathBuf::from("private/charts.bin");
    let key = PathBuf::from("private/key.rs");
    println!("cargo:rerun-if-changed=private/charts.bin");
    println!("cargo:rerun-if-changed=private/key.rs");

    if bin.exists() && key.exists() {
        fs::copy(&bin, out.join("charts.bin")).unwrap();
        // key.rs declares `pub(crate) const`s; strip the comment line pack.py adds.
        fs::copy(&key, out.join("key.rs")).unwrap();
    } else {
        println!("cargo:warning=private/charts.bin or private/key.rs not found: building without charts");
        fs::write(out.join("charts.bin"), b"OPB0").unwrap();
        fs::write(
            out.join("key.rs"),
            "pub(crate) const KA: [u8; 32] = [0; 32];\npub(crate) const KB: [u8; 32] = [0; 32];\n",
        )
        .unwrap();
    }
}
