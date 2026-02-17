# Windows Port Plan

Comprehensive plan for building and releasing Workbench on Windows.

---

## Phase 1: Build & Launch (Blockers)

These must be fixed before the app can even start on Windows.

### 1.1 PTY Shell Defaults — `src-tauri/src/pty.rs:76-100`

**Shell detection (line 76-80):** Currently defaults to `$SHELL` / `/bin/zsh`. Windows has no `SHELL` env var.

```rust
// Current
let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

// Fix: use COMSPEC on Windows
#[cfg(windows)]
{ std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string()) }
#[cfg(not(windows))]
{ std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()) }
```

**Login shell flag (line 83):** `-l` is meaningless to cmd.exe/PowerShell.

```rust
// Wrap with #[cfg(unix)]
#[cfg(unix)]
cmd.arg("-l");
```

**Environment variables (lines 86-100):** `HOME`, `USER`, `TERM`, `LANG` are Unix-only.

- Windows equivalents: `USERPROFILE`, `USERNAME`
- `TERM`/`COLORTERM` not used by Windows terminals (but ConPTY handles this)
- Gate the Unix vars behind `#[cfg(unix)]`, add `#[cfg(windows)]` block for `USERPROFILE`/`USERNAME`

### 1.2 Bundle Targets — `src-tauri/tauri.conf.json:30-44`

Currently only `["dmg", "app"]`. Add Windows targets:

```json
"targets": ["dmg", "app", "nsis"],
"windows": {
  "certificateThumbprint": null,
  "digestAlgorithm": "sha256",
  "allowMultipleInstallationPaths": true
}
```

NSIS is preferred over MSI for Tauri v2 (better UX, smaller installer, per-user install support).

### 1.3 Release Workflow — `.github/workflows/release.yml`

Currently macOS-only (`runs-on: macos-latest`, `--target universal-apple-darwin`). Need a matrix build:

```yaml
jobs:
  release:
    strategy:
      matrix:
        include:
          - os: macos-latest
            args: '--target universal-apple-darwin'
            rust-targets: aarch64-apple-darwin,x86_64-apple-darwin
          - os: windows-latest
            args: ''
            rust-targets: ''
    runs-on: ${{ matrix.os }}
```

Key differences for the Windows job:
- No `sed -i ''` (macOS syntax) — use platform-agnostic version bumping
- No `--target universal-apple-darwin` — use default target
- Signing: Windows code signing uses a certificate (`.pfx`), not Tauri signing keys. Optional but recommended for avoiding SmartScreen warnings
- `tauri-action` handles Windows builds natively — just remove the macOS-specific `args`

### 1.4 CI Workflow — `.github/workflows/ci.yml`

Add a Windows Rust test job:

```yaml
rust-windows:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: swatinem/rust-cache@v2
      with:
        workspaces: src-tauri
    - name: Run Rust tests
      run: cargo test
      working-directory: src-tauri
```

No system dependencies needed (unlike Linux's webkit2gtk). Windows uses WebView2 which ships with Windows 10/11.

---

## Phase 2: Core Features

These fix the main features that break on Windows paths/shells.

### 2.1 Path Encoding — `src-tauri/src/paths.rs:65-67`

`encode_project_path()` only replaces `/`. Windows paths have `\` and `:` (drive letter), both invalid in filenames.

```rust
pub fn encode_project_path(project_path: &str) -> String {
    project_path
        .replace('\\', "-")
        .replace('/', "-")
        .replace(':', "")
}
```

Affects: Claude session discovery, Trello project config, any encoded-path lookup.

### 2.2 Enriched PATH — `src-tauri/src/paths.rs:41-61`

Hardcoded macOS/Nix paths (`/opt/homebrew/bin`, etc.). On Windows these don't exist.

```rust
#[cfg(target_os = "macos")]
dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", ...]);

#[cfg(target_os = "windows")]
{
    // GitHub CLI, common tool locations
    if let Ok(pf) = std::env::var("ProgramFiles") {
        dirs.push(PathBuf::from(&pf).join("GitHub CLI"));
    }
}
```

Also fix the fallback from `/usr/bin:/bin` to `C:\Windows\System32` on Windows.

### 2.3 Shell Quoting — `src/lib/utils/claude.ts:43-56`

`shellSingleQuote()` uses bash-style `'...'` escaping. cmd.exe uses `"..."`, PowerShell uses different escape chars.

Options:
- **Detect shell type** in the frontend (pass shell info from backend) and quote accordingly
- **Move quoting to backend** where we know the shell
- **Use `--` separator** and let the CLI handle it (if Claude CLI supports it)

### 2.4 VS Code Launcher — `src-tauri/src/commands.rs:95-112`

`Command::new("code")` may not find `code.cmd` on Windows. Fix:

```rust
#[cfg(target_os = "windows")]
{
    std::process::Command::new("cmd.exe")
        .args(["/c", "code", &path])
        .spawn()?;
}
```

### 2.5 Worktree Paths — `src/lib/features/worktrees/WorktreeDialog.svelte:43-50`

Regex `/\/[^/]+$/` doesn't match backslashes. Path construction uses hardcoded `/`.

Fix: Use `lastIndexOf` for both `/` and `\\`, detect separator from the project path.

### 2.6 Terminal Font Stack — `src/lib/terminal-config.ts:32`

Add Windows fonts before the macOS-specific ones:

```typescript
fontFamily: 'JetBrains Mono, Cascadia Mono, Consolas, ui-monospace, SFMono-Regular, Menlo, monospace'
```

---

## Phase 3: Hook Bridge & Session Tracking

The hook bridge (`src-tauri/src/hook_bridge.rs`) is entirely disabled on non-Unix. This breaks real-time Claude/Codex session tracking.

### 3.1 Windows Hook Bridge Implementation

The Unix implementation uses `AF_UNIX` sockets. Two options for Windows:

**Option A: Named Pipes** (`\\.\pipe\workbench-hooks`)
- Native Windows IPC, no port conflicts
- Requires `windows-sys` or `winapi` crate
- Python client uses `open(r'\\.\pipe\workbench-hooks', 'r+b')`

**Option B: TCP localhost** (recommended — simpler)
- Bind `127.0.0.1:0` for an ephemeral port
- Store port in `WORKBENCH_HOOK_SOCKET` as `127.0.0.1:PORT`
- Python client uses `socket.AF_INET` + `socket.SOCK_STREAM`
- Works cross-platform — could even replace Unix sockets for consistency

### 3.2 Python Hook Scripts — `src-tauri/src/settings.rs:171-201`, `codex_config.rs:16-42`

Both scripts use `socket.AF_UNIX` which doesn't exist on Windows.

Fix: Add platform detection in the generated Python:

```python
import platform
if platform.system() == "Windows":
    host, port = socket_path.rsplit(":", 1)
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.connect((host, int(port)))
else:
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)
```

### 3.3 Script Execution — `src-tauri/src/paths.rs:71-86`

Shebangs (`#!/usr/bin/env python3`) are ignored on Windows. Hook registration must explicitly invoke Python:

```rust
#[cfg(windows)]
let command = format!("python \"{}\"", script_path.display());
```

---

## Phase 4: Robustness & Polish

### 4.1 Atomic Write — `src-tauri/src/paths.rs:91-99`

`fs::rename()` fails on Windows if the target file is open. Add a remove-before-rename fallback:

```rust
#[cfg(windows)]
if path.exists() {
    fs::remove_file(path)?;
}
fs::rename(&temp_path, path)?;
```

### 4.2 URL Opening — `src-tauri/src/github.rs:333-356`

Already has a `#[cfg(target_os = "windows")]` block but needs quoting fix for URLs with `&`:

```rust
Command::new("cmd").args(["/c", "start", "\"\"", url])
```

### 4.3 Test Portability

Several Rust tests use hardcoded Unix paths (`/home/user/...`). Use `cfg!(windows)` branches or `tempfile` for platform-agnostic assertions.

---

## Phase 5: Release Infrastructure

### 5.1 Code Signing (Optional but Recommended)

Without signing, Windows shows SmartScreen warnings ("Windows protected your PC"). To sign:

1. Obtain a code signing certificate (EV cert eliminates SmartScreen immediately; standard OV cert builds reputation over time)
2. Add to GitHub secrets: `WINDOWS_CERTIFICATE` (base64 `.pfx`), `WINDOWS_CERTIFICATE_PASSWORD`
3. Configure in `tauri.conf.json`:
   ```json
   "windows": {
     "certificateThumbprint": "<thumbprint>",
     "digestAlgorithm": "sha256",
     "timestampUrl": "http://timestamp.digicert.com"
   }
   ```

### 5.2 Auto-Updater

The updater is already configured with a signing key and endpoint. For Windows:
- `tauri-action` automatically generates `latest.json` with Windows assets
- The existing endpoint (`releases/latest/download/latest.json`) works for all platforms
- No additional config needed — Tauri's updater is cross-platform

### 5.3 WebView2 Runtime

Tauri v2 on Windows requires WebView2 (Evergreen runtime). It ships with Windows 10 (April 2018 update+) and Windows 11. The NSIS installer can optionally bundle the WebView2 bootstrapper for older systems:

```json
"nsis": {
  "installMode": "currentUser"
}
```

Tauri's NSIS template handles WebView2 detection and installation automatically.

### 5.4 Prerequisites for Windows Developers

Document in README:
- **Bun** (package manager)
- **Rust** (with MSVC toolchain — `rustup default stable-x86_64-pc-windows-msvc`)
- **Visual Studio Build Tools** (C++ workload — required by Rust on Windows)
- **WebView2** (pre-installed on modern Windows)
- **Git for Windows** (for git CLI)
- **GitHub CLI** (`gh`) for GitHub integration features
- **Python 3** for hook scripts

---

## Already Cross-Platform (No Changes Needed)

| Component | Why it works |
|-----------|-------------|
| `portable-pty` | Supports Windows ConPTY (Win10 1809+) |
| `notify` crate (file watcher) | Uses `ReadDirectoryChangesW` on Windows |
| `dirs` crate (home dir) | Returns `USERPROFILE` on Windows |
| Tauri plugins (dialog, store, shell, updater) | All cross-platform |
| Menu accelerators | `CmdOrCtrl` maps to Ctrl on Windows |
| `windows_subsystem = "windows"` attribute | Already in `main.rs` |
| `baseName()` utility | Already handles backslashes |
| Git CLI commands | `git` resolves to `git.exe` on Windows |
| `xterm.js` | Platform-agnostic terminal emulator |

---

## Summary

| Phase | Items | What it enables |
|-------|-------|----------------|
| 1 - Build & Launch | PTY defaults, bundle targets, CI/release workflows | App compiles and opens a terminal |
| 2 - Core Features | Path encoding, PATH enrichment, shell quoting, VS Code, worktrees, fonts | Projects, Claude sessions, GitHub integration |
| 3 - Hook Bridge | Windows IPC, Python scripts, script execution | Real-time session tracking |
| 4 - Polish | Atomic write, URL opening, test portability | Reliability, CI green on Windows |
| 5 - Release | Code signing, updater, WebView2, developer docs | Distribution to users |
