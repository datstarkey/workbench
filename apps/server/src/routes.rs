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

// --- control plane: projects / worktrees / branches ---

async fn list_projects() -> ApiResult<Json<Value>> {
    let projects = workbench_core::config::load_projects()?;
    Ok(Json(serde_json::to_value(projects)?))
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn list_worktrees(Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let worktrees = workbench_core::git::list_worktrees(&q.path)?;
    Ok(Json(serde_json::to_value(worktrees)?))
}

async fn create_worktree(Json(req): Json<CreateWorktreeRequest>) -> ApiResult<Json<Value>> {
    let path = workbench_core::git::create_worktree(&req)?;
    Ok(Json(json!({ "path": path })))
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
    workbench_core::git::remove_worktree(&body.repo_path, &body.worktree_path, body.force)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_branches(Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let branches = workbench_core::git::list_branches(&q.path)?;
    Ok(Json(serde_json::to_value(branches)?))
}

async fn git_info(Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let info = workbench_core::git::git_info(&q.path)?;
    Ok(Json(serde_json::to_value(info)?))
}

// --- control plane: session discovery (read-only) ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPathQuery {
    project_path: String,
}

async fn discover_claude_sessions(Query(q): Query<ProjectPathQuery>) -> ApiResult<Json<Value>> {
    let sessions = workbench_core::claude_sessions::discover_claude_sessions(&q.project_path)?;
    Ok(Json(serde_json::to_value(sessions)?))
}

async fn discover_codex_sessions(Query(q): Query<ProjectPathQuery>) -> ApiResult<Json<Value>> {
    let sessions = workbench_core::codex_sessions::discover_codex_sessions(&q.project_path)?;
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
    let value = workbench_core::settings::load_settings(&q.scope, q.project_path.as_deref())?;
    Ok(Json(value))
}

async fn load_workbench_settings() -> ApiResult<Json<Value>> {
    let settings = workbench_core::config::load_workbench_settings()?;
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
    let session = state.spawn.spawn(
        &body.project_path,
        body.worktree_path.as_deref(),
        body.name,
    )?;
    Ok(Json(serde_json::to_value(session)?))
}

async fn remote_sessions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.spawn.list()))
}

async fn remote_kill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state.spawn.kill(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
