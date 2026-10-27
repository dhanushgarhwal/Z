fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../../assets/icons/z.ico");
        res.set_manifest_file("../../assets/z.manifest");
        res.set("ProductName", "Z");
        res.set("FileDescription", "Z");
        res.set("CompanyName", "Z contributors");
        res.set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() { eprintln!("winres: {e}"); std::process::exit(1); }
    }
    println!("cargo:rerun-if-changed=../../assets/icons/z.ico");
}
