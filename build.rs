fn main() {
    println!("cargo:rerun-if-changed=assets/icons/wavehub.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/wavehub.ico");
        res.set("ProductName", "Wave Hub");
        res.set("FileDescription", "Wave Hub");
        res.set("CompanyName", "Derek Heasley");
        res.set("LegalCopyright", "Copyright (c) 2026 Derek Heasley. MIT License.");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon embedding skipped: {e}");
        }
    }
}
