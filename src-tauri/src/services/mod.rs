//! Application services, one per domain (PRD 6.2).
pub mod agents;
pub(crate) mod appdb;
pub(crate) mod attachments;
pub mod builds;
pub(crate) mod crafts;
pub mod diagnostics;
pub mod git;
pub(crate) mod github;
pub mod install;
pub mod jobs;
pub mod preview;
pub mod projects;
pub mod settings;
pub(crate) mod updates;
