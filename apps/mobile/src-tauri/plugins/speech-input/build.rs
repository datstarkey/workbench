fn main() {
    tauri_plugin::Builder::new(&["recognize"])
        .android_path("android")
        .build();
}
