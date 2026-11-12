fn main() {
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winres::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            res.set("ProductName", "CipherClipboard");
            res.set(
                "FileDescription",
                "CipherClipboard — Encrypted Cross-Platform Clipboard Manager",
            );
            res.set("LegalCopyright", "Copyright (c) 2026 BrunoO");
            if let Err(e) = res.compile() {
                eprintln!("Warning: Failed to compile Windows resource: {e}");
            }
        }
    }
}
