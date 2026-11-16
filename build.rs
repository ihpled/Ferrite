//! Build script for Ferrite
//!
//! This script handles platform-specific build tasks:
//! - Windows: Embeds the application icon into the executable

fn main() {
    // Windows: Embed icon into the executable.
    // Check the *target* OS (not the host's `cfg`), so the icon is also
    // embedded when cross-compiling for Windows from Linux or macOS.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let rc_path = std::path::Path::new("assets/icons/windows/app.rc");
        let ico_path = std::path::Path::new("assets/icons/windows/app.ico");

        // Only embed if both the .rc and .ico files exist
        if rc_path.exists() && ico_path.exists() {
            embed_resource::compile("assets/icons/windows/app.rc", embed_resource::NONE);
            println!("cargo:rerun-if-changed=assets/icons/windows/app.rc");
            println!("cargo:rerun-if-changed=assets/icons/windows/app.ico");
        } else {
            println!(
                "cargo:warning=Windows icon files not found at {:?} and {:?}. \
                 The executable will not have an embedded icon.",
                rc_path, ico_path
            );
        }
    }

    // Rerun build script if build.rs changes
    println!("cargo:rerun-if-changed=build.rs");
}
