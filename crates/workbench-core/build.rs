//! macOS: build the terminal exec shim (`src/pty_exec.c`) that `pty.rs` embeds.

fn main() {
    println!("cargo:rerun-if-changed=src/pty_exec.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out =
        std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("workbench-pty-exec");
    // The target's C compiler and flags (`-arch`, deployment target), linking
    // an executable rather than an object.
    let status = cc::Build::new()
        .opt_level(2)
        .debug(false)
        .get_compiler()
        .to_command()
        .arg("src/pty_exec.c")
        .arg("-o")
        .arg(&out)
        .status()
        .expect("failed to run the C compiler for src/pty_exec.c");
    assert!(status.success(), "compiling src/pty_exec.c failed");
}
