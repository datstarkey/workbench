fn main() {
    tauri_build::build();

    // Embed Windows manifest in test binaries.
    // Tauri embeds the manifest for application binaries but not tests.
    // Without it, Windows loads comctl32 v5 which lacks TaskDialogIndirect,
    // causing STATUS_ENTRYPOINT_NOT_FOUND at test startup.
    #[cfg(windows)]
    {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
