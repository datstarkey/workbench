use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use workbench_core::types::CreateWorktreeRequest;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/projects", get(list_projects))
        .route(
            "/projects/worktrees",
            get(list_worktrees)
                .post(create_worktree)
                .delete(remove_worktree),
        )
        .route("/projects/branches", get(list_branches))
        .route("/projects/git-info", get(git_info))
        .route("/sessions/claude", get(discover_claude_sessions))
        .route("/sessions/codex", get(discover_codex_sessions))
        .route("/settings/claude", get(load_claude_settings))
        .route("/settings/workbench", get(load_workbench_settings))
        .route("/settings/sync", put(settings_sync_stub))
        .route("/remote/spawn", post(remote_spawn))
        .route("/remote/sessions", get(remote_sessions))
        .route("/remote/sessions/:id", delete(remote_kill))
        .route(
            "/remote/terminals",
            get(crate::terminal::terminal_list).post(crate::terminal::terminal_create),
        )
        .route(
            "/remote/terminals/:id/ws",
            get(crate::terminal::terminal_attach),
        )
        .route(
            "/remote/terminals/:id",
            delete(crate::terminal::terminal_kill),
        )
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

/// Minimal mobile-friendly web client (spawn sessions / manage worktrees from a
/// phone browser over the private network). Complements the native mobile app.
async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

/// Run a blocking `workbench_core` call (git CLI, filesystem, PTY spawn) off the
/// async executor so it never stalls a tokio worker thread under concurrent load.
pub(crate) async fn blocking<T, F>(f: F) -> ApiResult<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("blocking task failed: {e}"),
        })?
        .map_err(ApiError::from)
}

// --- control plane: projects / worktrees / branches ---

async fn list_projects() -> ApiResult<Json<Value>> {
    let projects = blocking(workbench_core::config::load_projects).await?;
    Ok(Json(serde_json::to_value(projects)?))
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn list_worktrees(Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let worktrees = blocking(move || workbench_core::git::list_worktrees(&q.path)).await?;
    Ok(Json(serde_json::to_value(worktrees)?))
}

async fn create_worktree(Json(req): Json<CreateWorktreeRequest>) -> ApiResult<Json<String>> {
    // Returns the bare worktree path string to match the Tauri command and the
    // ControlPlaneCommands.create_worktree result type.
    let path = blocking(move || workbench_core::git::create_worktree(&req)).await?;
    Ok(Json(path))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveWorktreeBody {
    repo_path: String,
    worktree_path: String,
    #[serde(default)]
    force: bool,
}

async fn remove_worktree(Json(body): Json<RemoveWorktreeBody>) -> ApiResult<StatusCode> {
    blocking(move || {
        workbench_core::git::remove_worktree(&body.repo_path, &body.worktree_path, body.force)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_branches(Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let branches = blocking(move || workbench_core::git::list_branches(&q.path)).await?;
    Ok(Json(serde_json::to_value(branches)?))
}

async fn git_info(Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let info = blocking(move || workbench_core::git::git_info(&q.path)).await?;
    Ok(Json(serde_json::to_value(info)?))
}

// --- control plane: session discovery (read-only) ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPathQuery {
    project_path: String,
}

async fn discover_claude_sessions(Query(q): Query<ProjectPathQuery>) -> ApiResult<Json<Value>> {
    let sessions = blocking(move || {
        workbench_core::claude_sessions::discover_claude_sessions(&q.project_path)
    })
    .await?;
    Ok(Json(serde_json::to_value(sessions)?))
}

async fn discover_codex_sessions(Query(q): Query<ProjectPathQuery>) -> ApiResult<Json<Value>> {
    let sessions =
        blocking(move || workbench_core::codex_sessions::discover_codex_sessions(&q.project_path))
            .await?;
    Ok(Json(serde_json::to_value(sessions)?))
}

// --- control plane: settings (read-only here; sync is a future seam) ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsQuery {
    scope: String,
    project_path: Option<String>,
}

async fn load_claude_settings(Query(q): Query<SettingsQuery>) -> ApiResult<Json<Value>> {
    let value = blocking(move || {
        workbench_core::settings::load_settings(&q.scope, q.project_path.as_deref())
    })
    .await?;
    Ok(Json(value))
}

async fn load_workbench_settings() -> ApiResult<Json<Value>> {
    let settings = blocking(workbench_core::config::load_workbench_settings).await?;
    Ok(Json(serde_json::to_value(settings)?))
}

/// Seam for future cross-server settings sync. Intentionally unimplemented so the
/// route/shape exists without committing to the design yet.
async fn settings_sync_stub() -> ApiError {
    ApiError {
        status: StatusCode::NOT_IMPLEMENTED,
        message: "settings sync is not implemented yet".to_string(),
    }
}

// --- remote-control spawn (Claude only; Codex has no remote-control) ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpawnBody {
    project_path: String,
    worktree_path: Option<String>,
    name: Option<String>,
}

async fn remote_spawn(
    State(state): State<AppState>,
    Json(body): Json<SpawnBody>,
) -> ApiResult<Json<Value>> {
    if body.project_path.trim().is_empty() {
        return Err(ApiError::bad_request("projectPath is required"));
    }
    let spawn = state.spawn.clone();
    let session = blocking(move || {
        // Allowlist of registered project paths (loaded here, off the executor) so a
        // bare project_path can only resolve to a directory Workbench manages.
        let registered: Vec<String> = workbench_core::config::load_projects()?
            .into_iter()
            .map(|p| p.path)
            .collect();
        spawn.spawn(
            &body.project_path,
            body.worktree_path.as_deref(),
            body.name,
            &registered,
        )
    })
    .await?;
    Ok(Json(serde_json::to_value(session)?))
}

async fn remote_sessions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.spawn.list()))
}

async fn remote_kill(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    // Idempotent: a session that self-exited is reaped by its reader thread, so a
    // delete for an unknown id is a normal race, not a 500 (matches terminal_kill).
    state.spawn.kill(&id);
    StatusCode::NO_CONTENT
}
