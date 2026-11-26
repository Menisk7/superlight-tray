// Embeds the app icon + version info in the Windows exe. The icon is drawn by
// the same renderer the tray uses, so there is no binary asset in the repo.
#[allow(dead_code)]
mod icon {
    include!("src/icon.rs");
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    #[cfg(windows)]
    {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.ico");
        std::fs::write(&out, icon::ico(&[16, 24, 32, 48, 64, 256])).unwrap();
        let mut res = winres::WindowsResource::new();
        res.set_icon(out.to_str().unwrap());
        res.set("FileDescription", "Superlight Tray");
        res.set("ProductName", "Superlight Tray");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed icon: {e}");
        }
    }
}
