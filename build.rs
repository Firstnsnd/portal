//! Build script: embed the Windows VERSIONINFO resource (icon + product
//! metadata) when building ON Windows FOR Windows. The double gate matters:
//! `cargo check --target x86_64-pc-windows-msvc` from macOS has
//! CARGO_CFG_TARGET_OS=windows but a darwin host — winresource's
//! `compile()` needs the Windows SDK resource compiler (rc.exe), which only
//! exists on Windows hosts, so it must be skipped when cross-checking.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::consts::OS == "windows"
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/portal.ico");
        res.set("ProductName", "Portal");
        res.set("FileDescription", "Portal - terminal emulator with SSH/SFTP");
        let v = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        res.set("ProductVersion", &v);
        // VERSIONINFO requires a four-segment version; tag versions have three.
        res.set("FileVersion", &format!("{v}.0"));
        if let Err(e) = res.compile() {
            // Not fatal on CI unless the packaging step depends on it — but a
            // silently icon-less exe is confusing, so fail loudly.
            panic!("failed to compile Windows resource: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/portal.ico");
}
