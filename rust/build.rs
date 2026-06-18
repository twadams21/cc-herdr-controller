//! Locate the vendored SDL2 MinGW dev libs, tell the linker where they are, and
//! copy SDL2.dll next to the built binary so `cargo run` / the exe Just Work.
//!
//! We link SDL2 dynamically against `vendor/SDL2-*/x86_64-w64-mingw32`. Override
//! the lib dir with the SDL2_MINGW_DIR env var (must contain `lib/` and `bin/`).

use std::path::{Path, PathBuf};

fn find_mingw_dir(manifest: &Path) -> PathBuf {
    if let Ok(dir) = std::env::var("SDL2_MINGW_DIR") {
        return PathBuf::from(dir);
    }
    let vendor = manifest.join("vendor");
    if let Ok(entries) = std::fs::read_dir(&vendor) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("x86_64-w64-mingw32");
            if candidate.join("lib").is_dir() {
                return candidate;
            }
        }
    }
    panic!(
        "Could not find SDL2 MinGW libs under {}. Set SDL2_MINGW_DIR or run the \
         setup step that downloads SDL2-devel-*-mingw.",
        vendor.display()
    );
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mingw = find_mingw_dir(&manifest);

    println!("cargo:rustc-link-search=native={}", mingw.join("lib").display());
    println!("cargo:rerun-if-env-changed=SDL2_MINGW_DIR");
    println!("cargo:rerun-if-changed=build.rs");

    // Copy SDL2.dll next to the final binary (target/<profile>/SDL2.dll) so
    // `cargo run` and the shipped exe find it without extra setup.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    if let Some(profile_dir) = out_dir.ancestors().nth(3) {
        let dll_src = mingw.join("bin").join("SDL2.dll");
        let dll_dst = profile_dir.join("SDL2.dll");
        if dll_src.exists() {
            let _ = std::fs::copy(&dll_src, &dll_dst);
        }
    }
}
