use std::path::PathBuf;

use anyhow::Result;

const ZSH_INTEGRATION: &str = r#"
# Workbench shell integration (OSC 133)
# Restore original ZDOTDIR and source user's .zshrc
if [[ -n "$WORKBENCH_ORIG_ZDOTDIR" ]]; then
    ZDOTDIR="$WORKBENCH_ORIG_ZDOTDIR"
    unset WORKBENCH_ORIG_ZDOTDIR
elif [[ -n "$HOME" ]]; then
    ZDOTDIR="$HOME"
fi
[[ -f "$ZDOTDIR/.zshrc" ]] && source "$ZDOTDIR/.zshrc"

# ── OSC 133 FinalTerm hooks ──────────────────────────────────
__workbench_precmd() {
    local exit_code=$?
    if [[ -n "$__workbench_cmd_started" ]]; then
        builtin printf '\e]133;D;%s\a' "$exit_code"
        unset __workbench_cmd_started
    fi
    builtin printf '\e]133;A\a'
}

__workbench_preexec() {
    __workbench_cmd_started=1
    builtin printf '\e]133;C\a'
}

precmd_functions+=(__workbench_precmd)
preexec_functions+=(__workbench_preexec)

# Append B marker after prompt renders
PROMPT="${PROMPT}%{$(builtin printf '\e]133;B\a')%}"
"#;

pub fn ensure_shell_integration_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir()
        .join("workbench-shell-integration")
        .join("zsh");
    let rc_path = dir.join(".zshrc");
    // Only write if missing or content changed (avoid disk I/O on every spawn)
    let needs_write = match std::fs::read_to_string(&rc_path) {
        Ok(existing) => existing != ZSH_INTEGRATION,
        Err(_) => true,
    };
    if needs_write {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&rc_path, ZSH_INTEGRATION)?;
    }
    Ok(dir)
}
