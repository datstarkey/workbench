#[cfg(target_os = "macos")]
fn build_swift_bridge() {
    use std::env;
    use std::path::Path;
    use std::process::Command;

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let bridge_dir = Path::new(&manifest_dir).join("SwiftTermBridge");

    // Only build if the SwiftTermBridge directory exists
    if !bridge_dir.exists() {
        return;
    }

    // Determine the target architecture
    let target = env::var("TARGET").unwrap_or_default();
    let arch = if target.contains("aarch64") {
        "arm64"
    } else if target.contains("x86_64") {
        "x86_64"
    } else {
        "arm64" // default for modern macOS
    };

    // Build Swift package
    let status = Command::new("swift")
        .args(["build", "-c", "release", "--arch", arch])
        .current_dir(&bridge_dir)
        .status()
        .expect("Failed to run swift build. Is Swift installed?");

    if !status.success() {
        panic!("SwiftTermBridge build failed");
    }

    // Link the static library
    let lib_path = bridge_dir.join(".build/release");
    println!("cargo:rustc-link-search=native={}", lib_path.display());
    println!("cargo:rustc-link-lib=static=SwiftTermBridge");

    // Link required macOS frameworks
    for framework in &["AppKit", "Foundation", "CoreGraphics", "QuartzCore"] {
        println!("cargo:rustc-link-lib=framework={}", framework);
    }

    // Swift 5.0+ runtime is bundled with macOS, so we only need the SDK path
    // for the linker to find Swift runtime stubs.
    if let Ok(output) = Command::new("xcrun").args(["--show-sdk-path"]).output() {
        if let Ok(sdk_path) = String::from_utf8(output.stdout) {
            let sdk_path = sdk_path.trim();
            println!("cargo:rustc-link-search=native={}/usr/lib/swift", sdk_path);
        }
    }

    println!("cargo:rerun-if-changed=SwiftTermBridge/Sources/");
    println!("cargo:rerun-if-changed=SwiftTermBridge/Package.swift");
}

fn main() {
    #[cfg(target_os = "macos")]
    build_swift_bridge();

    tauri_build::build()
}
