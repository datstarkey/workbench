fn main() {
    tauri_plugin::Builder::new(&["start", "stop", "take_open_session", "register_listener", "remove_listener"])
        .android_path("android")
        .build();
}
