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
            // A missing rc.exe / windres used to leave the exe with no icon or
            // version resource while the build still reported success. That is
            // not a valid release: fail loudly instead of shipping a generic exe.
            res.compile().unwrap_or_else(|e| {
                panic!(
                    "Failed to compile Windows resources (icon/version info): {e}. \
                     Install the Windows SDK (rc.exe) or matching-architecture MinGW windres/ar."
                )
            });
        }
    }
}
