fn main() {
    tauri_plugin::Builder::new(&["start", "stop", "take_open_session"])
        .android_path("android")
        .build();
}
