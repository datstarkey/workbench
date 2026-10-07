use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

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
        .route("/projects/git-status", get(git_status))
        .route("/projects/git-diff", get(git_file_diff))
        .route("/projects/github-remote", get(github_remote))
        .route("/sessions/claude", get(discover_claude_sessions))
        .route("/sessions/codex", get(discover_codex_sessions))
        .route("/settings/claude", get(load_claude_settings))
        .route("/settings/workbench", get(load_workbench_settings))
        .route("/settings/sync", put(settings_sync_stub))
        .route(
            "/host/update",
            get(crate::host::update_status).post(crate::host::update_install),
        )
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
        .route("/mod/hello", post(crate::mod_routes::hello))
        .route("/mod/out", post(crate::mod_routes::out))
        .route("/mod/in", get(crate::mod_routes::poll))
        .route("/mod/ask", post(crate::mod_routes::ask))
        .route("/mod/bye", post(crate::mod_routes::bye))
        .route("/agent", get(crate::agent_routes::agent_list))
        .route(
            "/agent/attention",
            get(crate::agent_routes::agent_attention),
        )
        .route(
            "/agent/claude",
            get(crate::agent_routes::claude_list)
                .post(crate::agent_routes::agent_start)
                .delete(crate::agent_routes::agent_stop_pane),
        )
        .route(
            "/agent/codex",
            get(crate::agent_routes::codex_list)
                .post(crate::agent_routes::codex_start)
                .delete(crate::agent_routes::agent_stop_pane),
        )
        .route("/agent/usage", get(crate::agent_routes::agent_usage))
        .route("/agent/files", get(crate::agent_routes::agent_files))
        .route(
            "/agent/claude/:id/message",
            post(crate::agent_routes::agent_message),
        )
        .route(
            "/agent/claude/:id/tasks/:task_id/transcript",
            get(crate::agent_routes::agent_task_transcript),
        )
        .route("/agent/claude/:id", delete(crate::agent_routes::agent_stop))
        .route(
            "/agent/claude/:id/ws",
            get(crate::agent_routes::agent_attach),
        )
        .route(
            "/agent/codex/:id/message",
            post(crate::agent_routes::agent_message),
        )
        .route("/agent/codex/:id", delete(crate::agent_routes::agent_stop))
        .route(
            "/agent/codex/:id/ws",
            get(crate::agent_routes::agent_attach),
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitReviewQuery {
    project_path: String,
    worktree_path: Option<String>,
    file: Option<String>,
    #[serde(default)]
    staged: bool,
}

fn review_cwd(q: &GitReviewQuery) -> anyhow::Result<String> {
    crate::cwd::resolve_cwd(&q.project_path, q.worktree_path.as_deref())
}

async fn git_status(Query(q): Query<GitReviewQuery>) -> ApiResult<Json<Value>> {
    let status = blocking(move || workbench_core::git::git_status(&review_cwd(&q)?)).await?;
    Ok(Json(serde_json::to_value(status)?))
}

async fn git_file_diff(Query(q): Query<GitReviewQuery>) -> ApiResult<Json<String>> {
    if q.file.as_deref().is_none_or(str::is_empty) {
        return Err(ApiError::bad_request("file is required"));
    }
    let diff = blocking(move || {
        workbench_core::git::git_file_diff(&review_cwd(&q)?, q.file.as_deref().unwrap(), q.staged)
    })
    .await?;
    Ok(Json(diff))
}

/// `null` when the folder has no GitHub `origin` (not a repo, no remote, other host).
async fn github_remote(Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let remote = blocking(move || {
        Ok::<_, anyhow::Error>(workbench_core::github::get_github_remote(&q.path).ok())
    })
    .await?;
    Ok(Json(serde_json::to_value(remote)?))
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
    let mut settings = blocking(workbench_core::config::load_workbench_settings).await?;
    // The LAN token stays on this machine: a client holding one token must not
    // be able to read the current (possibly rotated) one.
    settings.server_token = None;
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
