//! A new session's first prompt as one argument for the terminal's shell,
//! shared by `claude_launch` and `codex_launch`. It always follows `--`, so a
//! prompt like `--dangerously-skip-permissions` stays a prompt, never a flag.

/// The prompt trimmed with line endings unified, or `None` when there's nothing to send.
pub fn normalize(prompt: Option<&str>) -> Option<String> {
    let prompt = prompt?.replace("\r\n", "\n").replace('\r', "\n");
    let prompt = prompt.trim();
    (!prompt.is_empty()).then(|| prompt.to_string())
}

/// The prompt as one shell argument, or `None` when it can't be one.
pub fn arg(prompt: &str) -> Option<String> {
    if cfg!(windows) {
        // cmd.exe and PowerShell share only double quotes, and neither honours
        // the other's escapes inside them: leave out whatever could end the
        // argument or expand rather than run something else.
        return (!prompt.contains(['"', '%', '$', '`', '!', '\n']))
            .then(|| format!("\"{prompt}\""));
    }
    Some(shell_quote(prompt))
}

/// Why `agent` started without its prompt (Windows refused it), to tell the person.
pub fn dropped_notice(agent: &str) -> String {
    format!(
        "{agent} started without its prompt: on Windows a prompt can't contain \" % $ ` ! \
         or line breaks. Paste it into {agent} instead."
    )
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}
