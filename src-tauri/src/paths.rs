use std::path::PathBuf;

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn workbench_config_dir() -> PathBuf {
    home_dir().join(".workbench")
}

pub fn claude_user_dir() -> PathBuf {
    home_dir().join(".claude")
}
