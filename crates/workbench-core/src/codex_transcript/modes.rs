//! Codex's approval + sandbox presets (its `/approvals` picker) and models.

use serde_json::{json, Value};

use crate::claude_transcript::{str_at, ModelOption};

/// `read-only`: read-only sandbox, asks on request. `auto`: workspace-write,
/// asks on request. `full-access`: no sandbox, never asks.
pub const CODEX_MODES: &[&str] = &["read-only", "auto", "full-access"];

/// A preset's `approvalPolicy` and `SandboxMode` (what `thread/start` takes).
pub fn sandbox_mode(mode: &str) -> Option<(&'static str, &'static str)> {
    match mode {
        "read-only" => Some(("on-request", "read-only")),
        "auto" => Some(("on-request", "workspace-write")),
        "full-access" => Some(("never", "danger-full-access")),
        _ => None,
    }
}

/// A preset's `SandboxPolicy` (what `turn/start` takes).
pub fn sandbox_policy(mode: &str) -> Option<Value> {
    Some(match mode {
        "read-only" => json!({"type": "readOnly", "networkAccess": false}),
        "auto" => json!({
            "type": "workspaceWrite",
            "writableRoots": [],
            "networkAccess": false,
            "excludeTmpdirEnvVar": false,
            "excludeSlashTmp": false,
        }),
        "full-access" => json!({"type": "dangerFullAccess"}),
        _ => return None,
    })
}

/// The preset an effective `approvalPolicy` + sandbox type amounts to, if any.
pub fn mode_of(approval_policy: &Value, sandbox_type: &str) -> Option<&'static str> {
    match (approval_policy.as_str()?, sandbox_type) {
        ("on-request", "readOnly") => Some("read-only"),
        ("on-request", "workspaceWrite") => Some("auto"),
        ("never", "dangerFullAccess") => Some("full-access"),
        _ => None,
    }
}

/// `model/list` entries as picker options.
pub(super) fn model_options(data: &[Value]) -> Vec<ModelOption> {
    data.iter()
        .filter(|m| m.get("hidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|m| {
            Some(ModelOption {
                value: str_at(m, "id")?.to_string(),
                default_effort: str_at(m, "defaultReasoningEffort").map(String::from),
                input_modalities: m
                    .get("inputModalities")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect(),
                service_tiers: m
                    .get("serviceTiers")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                display_name: str_at(m, "displayName").unwrap_or_default().to_string(),
                description: str_at(m, "description").unwrap_or_default().to_string(),
                resolved_model: str_at(m, "model").map(String::from),
                effort_levels: m
                    .get("supportedReasoningEfforts")
                    .and_then(Value::as_array)
                    .map(|l| {
                        l.iter()
                            .filter_map(|e| str_at(e, "reasoningEffort"))
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}
