use std::{env, fs, path::PathBuf};

// Z Setup carries Z.exe and zc.exe inside itself. They are copied from the release build folder
// (override with Z_PAYLOAD_DIR). When they are not built yet, empty placeholders keep
// `cargo test --workspace` working; the installer then refuses to run (see install.rs).
fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("..").join("..");
    let dir = env::var("Z_PAYLOAD_DIR").map(PathBuf::from).unwrap_or_else(|_| root.join("target").join("release"));
    for name in ["Z.exe", "zc.exe"] {
        let src = dir.join(name);
        let dst = out.join(name);
        if fs::copy(&src, &dst).is_err() { fs::write(&dst, b"").expect("write placeholder"); }
        println!("cargo:rerun-if-changed={}", src.display());
    }
    println!("cargo:rerun-if-env-changed=Z_PAYLOAD_DIR");
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../../assets/icons/z.ico");
        res.set_manifest_file("../../assets/z.manifest");
        res.set("ProductName", "Z");
        res.set("FileDescription", "Z Setup");
        res.set("CompanyName", "Z contributors");
        res.set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() { eprintln!("winres: {e}"); std::process::exit(1); }
    }
}
