const COMMANDS: &[&str] = &["download_and_install"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
