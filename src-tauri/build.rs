fn main() {
    tauri_build::build();

    // Embed Windows manifest in test binaries.
    // Tauri embeds the manifest for the main binary but not tests.
    // Without it, Windows loads comctl32 v5 which lacks TaskDialogIndirect,
    // causing STATUS_ENTRYPOINT_NOT_FOUND at test startup.
    // Only applied during `cargo test` (CARGO_CFG_TEST is set) to avoid
    // interfering with Tauri's own manifest in normal builds.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TEST").is_ok() {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
