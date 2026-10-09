//! The workspace → tab → pane model every client renders, the commands that
//! change it, and its file. See `docs/WORKSPACE_MODEL.md`.

pub mod command;
pub mod model;
pub mod ops;
pub mod persist;

pub use command::{Command, Effect, Target};
pub use model::*;
pub use ops::apply;
