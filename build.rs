fn main() {
    println!("cargo:rerun-if-changed=assets/icons/wavehub.ico");
    println!("cargo:rerun-if-changed=assets/wavehub.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/wavehub.ico");
        // Declares "runs as the user, no admin" and Windows 10/11 support, like signed apps do.
        let manifest = std::fs::read_to_string("assets/wavehub.manifest")
            .expect("assets/wavehub.manifest")
            .replace("@VERSION@", env!("CARGO_PKG_VERSION"));
        let manifest_path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("wavehub.manifest");
        std::fs::write(&manifest_path, manifest).expect("write manifest");
        res.set_manifest_file(manifest_path.to_str().unwrap());
        res.set("OriginalFilename", "WaveHub.exe");
        res.set("InternalName", "WaveHub");
        res.set("ProductName", "Wave Hub");
        res.set("FileDescription", "Wave Hub");
        res.set("CompanyName", "Del (postmanpat)");
        res.set("LegalCopyright", "Copyright (c) 2026 Del (postmanpat). MIT License.");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon embedding skipped: {e}");
        }
    }
}
