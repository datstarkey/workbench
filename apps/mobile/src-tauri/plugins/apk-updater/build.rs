const COMMANDS: &[&str] = &[
    "download_and_install",
    "install_downloaded",
    "clear_downloads",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
