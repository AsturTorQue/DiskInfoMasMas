// Puts the icon and version details into DiskInfo.exe when building on Windows.
fn main() {
    println!("cargo:rerun-if-changed=packaging/diskinfo.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/diskinfo.ico");
        res.set("ProductName", "DiskInfo");
        res.set("FileDescription", "DiskInfo - disk health and temperature");
        res.set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon not embedded: {e}");
        }
    }
}
